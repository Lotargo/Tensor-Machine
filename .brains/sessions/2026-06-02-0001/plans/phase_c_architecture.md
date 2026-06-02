# Phase C: Horizontal Scaling — Architectural Breakdown

**Date:** 2026-06-02
**Corresponds to:** Tasks 4.3.1–4.3.4
**Goal:** Network-transparent multi-instance MCP with Arrow Flight data plane, load-balanced HTTP/WS gateway, sticky sessions, and a 50k req/s stress test.

---

## 1. Current State (Single-Node Bottleneck)

```
Client
  │
  ▼
┌──────────────────┐
│  MCP Server      │  (main.rs:52 — single-threaded loop)
│  ┌────────────┐  │
│  │ Planner    │  │  (ir.rs — pure, compiles AST → Program)
│  ├────────────┤  │
│  │ VM::step() │  │  (vm.rs — stack-based, in-process)
│  ├────────────┤  │
│  │ Sessions   │  │  (mcp.rs:173 — local HashMap<String, McpSession>)
│  ├────────────┤  │
│  │ DbPools    │  │  (db.rs — named backends, circuit breakers)
│  └────────────┘  │
└──────────────────┘
       ▲
       │  mpsc channel (capacity 100)
       │
   main.rs:47  (_tx, rx) — dead code, no actual network listener
```

### Concrete constraints

| Constraint | Current | Target |
|-----------|---------|--------|
| Request handling | Sequential `loop { select! { req = rx.recv() } }` | Concurrent, multi-worker |
| Network transport | None — `mpsc` is in-process only | HTTP/WS + Arrow Flight |
| Session storage | `HashMap` in process memory | Distributed, sticky-routed |
| Max throughput | ~1k req/s (single-threaded estimate) | 50k req/s multi-instance |
| Data exchange | Same-process `Arc<RecordBatch>` | Arrow Flight plane across instances |
| State durability | Lost on restart | Survives worker failure (via Redis) |

---

## 2. Target Architecture

```
                        ┌──────────────────────┐
                        │  HTTP/WS Gateway      │
                        │  (axum, port 8080)    │
                        │                       │
                        │  Consistent Hash      │
                        │  Ring: session_id →   │
                        │    worker_index        │
                        └──────┬────────────────┘
                               │
              ┌────────────────┼────────────────┐
              │                │                │
              ▼                ▼                ▼
     ┌──────────────┐ ┌──────────────┐ ┌──────────────┐
     │ MCP Worker 0 │ │ MCP Worker 1 │ │ MCP Worker 2 │  (N workers, each on port 9091+i)
     │              │ │              │ │              │
     │ Planner     │ │ Planner      │ │ Planner      │
     │ VM          │ │ VM           │ │ VM           │
     │ Sessions    │ │ Sessions     │ │ Sessions     │
     │ (local      │ │ (local       │ │ (local       │
     │  cache)     │ │  cache)      │ │  cache)      │
     └──────┬───────┘ └──────┬───────┘ └──────┬───────┘
            │                │                │
            └────────────────┼────────────────┘
                             │
                    ┌────────▼────────┐
                    │ Redis Cluster   │
                    │ (session store, │
                    │  result cache)  │
                    └─────────────────┘

Arrow Flight Data Plane (port 50051 per worker):
  ┌──────────┐     ┌──────────┐     ┌──────────┐
  │ Worker 0 │◄───►│ Worker 1 │◄───►│ Worker 2 │
  └──────────┘     └──────────┘     └──────────┘
     Flight service serves RecordBatch data
     so workers can exchange Arrow data directly
```

### Two-layer scaling model

**Layer 1: Gateway (stateless, horizontally scalable)**
- Axum HTTP/WS server, accepts MCP JSON requests
- Routes to worker via consistent hash of `session_id`
- No session state — pure routing
- Proxies responses back to client

**Layer 2: Workers (stateful, behind gateway)**
- Each worker runs MCP server + Planner + VM + DbPools
- Local session cache (hot path, best-effort)
- Redis backing store (authoritative session state + result cache)
- Arrow Flight service for cross-worker data exchange (e.g., `colbert_tokens` for re-ranking across shards)

---

## 3. Component Breakdown

### 3.1 Arrow Flight Data Plane (Task 4.3.1)

**New file:** `tensor-rag/src/flight.rs`
**Dep added:** `arrow-flight = "51.0.0"` (matching existing arrow = "51.0.0")

Arrow Flight is a gRPC-based protocol for high-throughput transfer of Arrow `RecordBatch` data. Each worker exposes a Flight service that allows other workers to pull/push Arrow data directly.

#### FlightService Implementation

```rust
use arrow_flight::{
    flight_service_server::{FlightService, FlightServiceServer},
    Action, ActionType, Criteria, Empty, FlightData, FlightDescriptor,
    FlightInfo, HandshakeRequest, HandshakeResponse, PollInfo, PutResult,
    SchemaResult, Ticket,
};
use tonic::{Request, Response, Status, Streaming};

#[derive(Clone)]
pub struct TensorFlightService {
    shm_manager: Arc<SharedMemoryManager>,
    db_pools: Arc<DbPools>,
}

#[tonic::async_trait]
impl FlightService for TensorFlightService {
    /// Client wants to pull data: returns FlightData stream
    async fn do_get(
        &self,
        request: Request<Ticket>,
    ) -> Result<Response<Self::DoGetStream>, Status> {
        // Ticket.ticket contains:
        //   "qdrant:{session_id}:{top_k}"      → search Qdrant, return RecordBatch
        //   "postgres:{session_id}:{id_list}"  → fetch BM25 scores, return RecordBatch
        //   "shm:{region_name}"                → read from shared memory region
        let ticket = std::str::from_utf8(&request.get_ref().ticket)
            .map_err(|_| Status::invalid_argument("Invalid ticket UTF-8"))?;

        // Parse ticket prefix
        if let Some(shm_region) = ticket.strip_prefix("shm:") {
            // Zero-copy path: read RecordBatch from shared memory
            let batch = crate::arrow::read_batch_from_shm(&self.shm_manager, shm_region)
                .map_err(|e| Status::internal(e))?;
            let mut writer = arrow_flight::utils::FlightDataEncoderBuilder::new()
                .build()
                .into_stream(vec![batch]);
            // ...
        } else if let Some(qdrant_key) = ticket.strip_prefix("qdrant:") {
            // Network-fetch path: execute Qdrant search on behalf of another worker
            let parts: Vec<&str> = qdrant_key.split(':').collect();
            let (session_id, top_k_str) = (parts[0], parts[1]);
            let top_k: usize = top_k_str.parse().map_err(|_| Status::invalid_argument("top_k"))?;

            let query_vec = ndarray::Array1::from_elem(768, 0.1);
            let (ids, vectors) = self.db_pools.search_qdrant(&query_vec, &None, top_k)
                .await
                .map_err(|e| Status::internal(e))?;

            let batch = crate::arrow::qdrant_to_record_batch(&ids, &vectors)
                .map_err(|e| Status::internal(e.to_string()))?;

            let mut writer = arrow_flight::utils::FlightDataEncoderBuilder::new()
                .build()
                .into_stream(vec![batch]);
            // ...
        }
        // ...
    }

    /// Client wants to push data: receives FlightData stream and stores it
    async fn do_put(
        &self,
        request: Request<Streaming<FlightData>>,
    ) -> Result<Response<Self::DoPutStream>, Status> {
        // Receive RecordBatch, write to shared memory, return region name
        // ...
    }

    /// Return available data "tickets" / endpoints
    async fn get_flight_info(
        &self,
        _request: Request<FlightDescriptor>,
    ) -> Result<Response<FlightInfo>, Status> {
        // Advertise available data catalogs
        // ...
    }
}
```

#### Ticket Protocol

```
do_get("qdrant:{session_id}:{top_k}")
  → Searches Qdrant (or reads local cache)
  → Returns RecordBatch with columns: id, vector, bm25_score, text, colbert_tokens

do_get("postgres:{session_id}:{ids_csv}")
  → Fetches BM25 scores + text metadata for given IDs from Postgres
  → Returns RecordBatch with same schema

do_get("shm:{region_name}")
  → Zero-copy: reads pre-written RecordBatch from shared memory
  → Returns FlightData stream

do_put("batch:{session_id}:{sequence}")
  → Receives RecordBatch from another worker
  → Writes to local shared memory for local VM consumption
```

#### Worker Registration

Each worker starts its Flight service on `0.0.0.0:50051` (configurable via env `FLIGHT_BIND_ADDR`). Workers discover each other via a shared Redis registry or static config.

```rust
// flight.rs
pub async fn start_flight_service(
    service: TensorFlightService,
    port: u16,
    cancel: CancellationToken,
) -> Result<(), Box<dyn std::error::Error>> {
    let addr = format!("0.0.0.0:{}", port).parse()?;
    let svc = FlightServiceServer::new(service);
    tonic::transport::Server::builder()
        .add_service(svc)
        .serve_with_shutdown(addr, async { cancel.cancelled().await; })
        .await?;
    Ok(())
}
```

---

### 3.2 HTTP/WebSocket Gateway (Task 4.3.2)

**Files modified:** `main.rs`, new `gateway.rs` (or inline in `main.rs`)

The gateway is a stateless axum server that:
1. Accepts MCP JSON requests via HTTP POST `/api/mcp` or WebSocket `/ws`
2. Determines target worker via consistent hash of `session_id`
3. Proxies the request to the worker's inner HTTP server
4. Returns the response to the client

#### Request Flow

```
Client POST /api/mcp { "tool": "search", "args": {...}, "session_id": "abc", "sequence_number": 1 }

1. Gateway receives request
2. Computes hash("abc") % N_workers → worker_index
3. Forwards to http://worker_{idx}:9091/mcp via reqwest
4. Waits for response
5. Returns to client
```

#### Gateway Implementation

```rust
// gateway.rs (or main.rs)

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub struct GatewayConfig {
    pub workers: Vec<String>,  // e.g. ["http://127.0.0.1:9091", "http://127.0.0.1:9092", ...]
    pub http_client: reqwest::Client,
}

pub fn select_worker(session_id: &str, workers: &[String]) -> usize {
    let mut hasher = DefaultHasher::new();
    session_id.hash(&mut hasher);
    let hash = hasher.finish();
    (hash as usize) % workers.len()
}

// axum handler
async fn mcp_handler(
    State(gw): State<Arc<GatewayConfig>>,
    Json(req): Json<McpRequest>,
) -> Result<Json<McpResponse>, StatusCode> {
    let idx = select_worker(&req.session_id, &gw.workers);
    let worker_url = format!("{}/mcp", gw.workers[idx]);

    let resp = gw.http_client
        .post(&worker_url)
        .json(&req)
        .send()
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;

    let status = resp.status();
    let body = resp.bytes().await.map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;

    if status.is_success() {
        Ok(Json(serde_json::from_slice(&body).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?))
    } else {
        Err(StatusCode::BAD_GATEWAY)
    }
}
```

#### Worker Server Changes

Each worker also runs its own axum server (different port from the metrics server) that accepts forwarded MCP requests from the gateway:

```rust
// main.rs — worker mode
async fn start_worker_server(
    db_pools: Arc<DbPools>,
    metrics: Option<Arc<Metrics>>,
    port: u16,
    cancel: CancellationToken,
) -> Result<(), std::io::Error> {
    let mut server = McpServer::new(db_pools, metrics);
    register_search_tool(&mut server);
    let server = Arc::new(Mutex::new(server));

    let app = Router::new()
        .route("/mcp", post(mcp_handler_worker))
        .with_state(server);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(async move { cancel.cancelled().await; })
        .await
}
```

#### WebSocket Support

For long-lived MCP connections (streaming results, tool notifications):

```rust
async fn ws_handler(
    State(gw): State<Arc<GatewayConfig>>,
    ws: WebSocketUpgrade,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let session_id = params.get("session_id").cloned().unwrap_or_default();
    let idx = select_worker(&session_id, &gw.workers);
    let worker_url = gw.workers[idx].clone();
    ws.on_upgrade(move |socket| proxy_ws(socket, worker_url, session_id))
}

async fn proxy_ws(client_ws: WebSocket, worker_url: String, session_id: String) {
    // Connect to worker_ws_server via tokio_tungstenite
    // Bidirectional forward messages
}
```

---

### 3.3 Session Affinity via Consistent Hashing (Task 4.3.3)

#### Consistent Hash Ring

A `session_id` → worker mapping is computed at the gateway using consistent hashing. This ensures:
- Same session always hits the same worker (sticky session)
- Adding/removing workers only remaps a fraction of sessions (minimal disruption)

We implement a simple ring with virtual nodes (no external dep needed — re-use `std::hash::Hasher`):

```rust
// flight.rs or new consistent_hash.rs

pub struct ConsistentHashRing {
    ring: BTreeMap<u64, usize>,  // hash → worker_index
    workers: Vec<String>,
    virtual_nodes: usize,  // replicas per worker (e.g., 100)
}

impl ConsistentHashRing {
    pub fn new(workers: Vec<String>, virtual_nodes: usize) -> Self {
        let mut ring = BTreeMap::new();
        for (i, worker) in workers.iter().enumerate() {
            for v in 0..virtual_nodes {
                let mut hasher = DefaultHasher::new();
                format!("{}:{}", worker, v).hash(&mut hasher);
                ring.insert(hasher.finish(), i);
            }
        }
        Self { ring, workers, virtual_nodes }
    }

    pub fn get(&self, key: &str) -> &str {
        if self.ring.is_empty() {
            panic!("No workers in ring");
        }
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let hash = hasher.finish();

        // Find the first node >= hash, wrapping around
        let idx = self.ring.range(hash..)
            .next()
            .map(|(_, &idx)| idx)
            .unwrap_or_else(|| *self.ring.first_key_value().unwrap().1);

        &self.workers[idx]
    }

    /// Add a worker without full rebuild (fractional remapping)
    pub fn add_worker(&mut self, worker: String) {
        let idx = self.workers.len();
        self.workers.push(worker.clone());
        for v in 0..self.virtual_nodes {
            let mut hasher = DefaultHasher::new();
            format!("{}:{}", worker, v).hash(&mut hasher);
            self.ring.insert(hasher.finish(), idx);
        }
    }
}
```

#### Session Cache on Worker

Each worker maintains a two-tier session store:

```rust
// mcp.rs — in McpServer

pub struct McpServer {
    // ... existing fields ...

    // NEW: two-tier session store
    session_cache: HashMap<String, McpSession>,     // local hot cache
    session_store: Option<Arc<RedisSessionStore>>,  // Redis backing (optional)
}

impl McpServer {
    pub async fn get_session(&mut self, session_id: &str) -> Result<&mut McpSession, String> {
        // 1. Check local cache first (fast path)
        if self.session_cache.contains_key(session_id) {
            return Ok(self.session_cache.get_mut(session_id).unwrap());
        }

        // 2. Check Redis (slow path — session was migrated or worker restarted)
        if let Some(store) = &self.session_store {
            if let Some(session) = store.load(session_id).await? {
                self.session_cache.insert(session_id.to_string(), session);
                return Ok(self.session_cache.get_mut(session_id).unwrap());
            }
        }

        // 3. Create new session locally + persist to Redis
        let (tx, rx) = mpsc::channel(100);
        let session = McpSession::new(session_id.to_string(), tx);
        if let Some(store) = &self.session_store {
            store.save(session_id, &session).await?;
        }
        self.session_cache.insert(session_id.to_string(), session);
        Ok(self.session_cache.get_mut(session_id).unwrap())
    }
}
```

#### Session Recovery

If a worker crashes and restarts:
1. Gateway's consistent hash still points `session_id` to the same worker index
2. Worker's local cache is empty (cold start)
3. `get_session` falls through to Redis → loads session state (expected_sequence number)
4. Client reconnects with next `sequence_number` → message ordering is preserved
5. If Redis is also down: session is lost, client gets `Session not found` and must re-establish

---

### 3.4 Distributed Session Store (Task 4.3.3 — Redis)

**New file:** `tensor-rag/src/session_store.rs`
**Dep added:** `redis = { version = "0.27", features = ["tokio", "connection-manager"] }`

#### RedisSessionStore

```rust
pub struct RedisSessionStore {
    client: redis::aio::ConnectionManager,
    key_prefix: String,
    ttl_secs: usize,  // session TTL (e.g., 3600 = 1 hour)
}

impl RedisSessionStore {
    pub async fn new(redis_url: &str) -> Result<Self, redis::RedisError> {
        let client = redis::Client::open(redis_url)?;
        let connection = client.get_connection_manager().await?;
        Ok(Self {
            client: connection,
            key_prefix: "tensor_rag:session:".to_string(),
            ttl_secs: 3600,
        })
    }

    pub async fn save(&self, session_id: &str, session: &McpSession) -> Result<(), String> {
        let key = format!("{}{}", self.key_prefix, session_id);
        let data = serde_json::to_vec(session).map_err(|e| e.to_string())?;
        redis::cmd("SETEX")
            .arg(&key)
            .arg(self.ttl_secs)
            .arg(&data)
            .query_async(&mut self.client.clone())
            .await
            .map_err(|e: redis::RedisError| e.to_string())
    }

    pub async fn load(&self, session_id: &str) -> Result<Option<McpSession>, String> {
        let key = format!("{}{}", self.key_prefix, session_id);
        let data: Option<Vec<u8>> = redis::cmd("GET")
            .arg(&key)
            .query_async(&mut self.client.clone())
            .await
            .map_err(|e: redis::RedisError| e.to_string())?;

        match data {
            Some(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|e| e.to_string()),
            None => Ok(None),
        }
    }

    pub async fn delete(&self, session_id: &str) -> Result<(), String> {
        let key = format!("{}{}", self.key_prefix, session_id);
        redis::cmd("DEL")
            .arg(&key)
            .query_async(&mut self.client.clone())
            .await
            .map_err(|e: redis::RedisError| e.to_string())
    }
}
```

#### Result Cache (Optional Optimization)

In addition to sessions, Redis can cache `FlatBuffer` results keyed by query hash:

```
Key: tensor_rag:cache:{query_hash}:{rerank_strategy_hash}:{top_k}
Value: FlatBuffer bytes (same format as McpResponse.body)
TTL: 60 seconds (configurable)
```

This prevents re-executing identical queries within the TTL window. The cache is checked before `Planner::compile` and populated after `execute_program`.

---

### 3.5 Stress Test 50k req/s (Task 4.3.4)

**New file:** `tensor-rag/benches/stress_test.rs`
**Dep added (dev):** `reqwest = { version = "0.12", features = ["json"] }` (for HTTP load generation)

#### Test Design

The stress test targets the HTTP gateway (not the workers directly) to validate the full path: gateway → consistent hash → worker → Planner → VM → response.

```rust
// benches/stress_test.rs (harness = false)
// Not a criterion bench — it's a standalone throughput test

use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;

const TARGET_RPS: usize = 50_000;
const DURATION_SECS: u64 = 10;
const CONCURRENCY: usize = 500;  // 500 concurrent connections

#[tokio::main]
async fn main() {
    let client = reqwest::Client::new();
    let semaphore = Arc::new(Semaphore::new(CONCURRENCY));
    let completed = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let errors = Arc::new(std::sync::atomic::AtomicU64::new(0));

    let start = Instant::now();
    let mut handles = Vec::new();

    // Pre-generate 10k unique session IDs for realistic distribution
    let session_ids: Vec<String> = (0..10_000)
        .map(|i| format!("stress_session_{}", i))
        .collect();

    // Spawn workers that fire requests as fast as semaphore allows
    for _ in 0..CONCURRENCY {
        let client = client.clone();
        let sem = semaphore.clone();
        let completed = completed.clone();
        let errors = errors.clone();
        let sessions = session_ids.clone();

        handles.push(tokio::spawn(async move {
            loop {
                let _permit = sem.acquire().await.unwrap();
                let session_id = &sessions[fastrand::usize(..sessions.len())];

                let req_json = serde_json::json!({
                    "tool": "search",
                    "args": {
                        "query": "stress test query",
                        "filters": null,
                        "rerank_strategy": {
                            "type": "hybrid",
                            "semantic_weight": 0.7,
                            "bm25_weight": 0.3
                        },
                        "final_top_k": 5
                    },
                    "session_id": session_id,
                    "sequence_number": 1,
                });

                match client
                    .post("http://127.0.0.1:8080/api/mcp")
                    .json(&req_json)
                    .timeout(std::time::Duration::from_secs(5))
                    .send()
                    .await
                {
                    Ok(resp) => {
                        if resp.status().is_success() {
                            completed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        } else {
                            errors.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                    Err(_) => {
                        errors.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                }
                drop(_permit);
            }
        }));
    }

    // Let it run for DURATION_SECS
    tokio::time::sleep(Duration::from_secs(DURATION_SECS)).await;

    // Report
    let elapsed = start.elapsed().as_secs_f64();
    let total = completed.load(std::sync::atomic::Ordering::Relaxed);
    let errs = errors.load(std::sync::atomic::Ordering::Relaxed);
    let rps = total as f64 / elapsed;

    println!("=== Stress Test Results ===");
    println!("Duration: {:.1}s", elapsed);
    println!("Successful requests: {}", total);
    println!("Failed requests: {}", errs);
    println!("Throughput: {:.0} req/s", rps);
    println!("Target: 50,000 req/s");
    println!("Status: {}", if rps >= 50_000.0 { "✅ PASS" } else { "❌ FAIL" });
}
```

#### Test Modes

| Mode | Description |
|------|-------------|
| `direct_inprocess` | Direct mpsc channel, no network — baseline for max single-node throughput |
| `direct_http_worker` | Single worker via HTTP — measures HTTP overhead |
| `gateway_single_worker` | Gateway → 1 worker — measures gateway proxying overhead |
| `gateway_3_workers` | Gateway → 3 workers — measures distribution + sticky sessions |
| `gateway_3_workers_redis` | Full stack with Redis session store |

**Success criteria:** Gateway + 3 workers + Redis achieves ≥50k req/s sustained for 10s with <0.1% error rate.

#### Infrastructure Requirements

To run the stress test:
```bash
# Start Redis
redis-server --port 6379 &

# Start 3 workers
WORKER_ID=0 MCP_PORT=9091 FLIGHT_PORT=50051 REDIS_URL=redis://127.0.0.1:6379 cargo run -- worker &
WORKER_ID=1 MCP_PORT=9092 FLIGHT_PORT=50052 REDIS_URL=redis://127.0.0.1:6379 cargo run -- worker &
WORKER_ID=2 MCP_PORT=9093 FLIGHT_PORT=50053 REDIS_URL=redis://127.0.0.1:6379 cargo run -- worker &

# Start gateway
GATEWAY_WORKERS="http://127.0.0.1:9091,http://127.0.0.1:9092,http://127.0.0.1:9093" cargo run -- gateway &

# Run stress test
cargo run --release --bench stress_test
```

---

## 4. Configuration & Startup

### main.rs — Mode Dispatch

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(|s| s.as_str()).unwrap_or("gateway");

    match mode {
        "gateway" => run_gateway().await?,
        "worker" => run_worker().await?,
        _ => run_gateway().await?,  // default
    }
    Ok(())
}
```

### Environment Variables

| Variable | Default | Used By | Description |
|----------|---------|---------|-------------|
| `MCP_PORT` | `9091` | worker | HTTP port for worker's inner MCP server |
| `GATEWAY_PORT` | `8080` | gateway | HTTP/WS port for gateway |
| `FLIGHT_PORT` | `50051` | worker | Arrow Flight gRPC port |
| `METRICS_PORT` | `9090` | worker | Prometheus /health /ready port |
| `GATEWAY_WORKERS` | — | gateway | Comma-separated worker URLs |
| `REDIS_URL` | — | worker | Redis connection string for session store |
| `WORKER_ID` | `0` | worker | Unique worker identifier |
| `DATA_DIR` | `/tmp/tensor_rag` | worker | Shared memory tmpfs directory |

### Startup Lifecycle

```
Gateway startup:
  1. Parse GATEWAY_WORKERS → Vec<url>
  2. Build ConsistentHashRing
  3. Start axum HTTP/WS server on GATEWAY_PORT
  4. Wait for Ctrl-C → graceful shutdown

Worker startup:
  1. Init tracing, metrics, DbPools
  2. Connect to Redis (if REDIS_URL set) → RedisSessionStore
  3. Create McpServer with session_store
  4. Start inner HTTP server on MCP_PORT (for gateway proxying)
  5. Start Arrow Flight service on FLIGHT_PORT
  6. Start metrics server on METRICS_PORT
  7. Wait for Ctrl-C → graceful shutdown (drain active, close sockets, cleanup shm)
```

---

## 5. File Change Summary

| File | Action | Description |
|------|--------|-------------|
| `tensor-rag/src/flight.rs` | **CREATE** | Arrow Flight service: `TensorFlightService`, `start_flight_service`, ticket protocol |
| `tensor-rag/src/session_store.rs` | **CREATE** | `RedisSessionStore`: save/load/delete sessions, optional result cache |
| `tensor-rag/src/consistent_hash.rs` | **CREATE** | `ConsistentHashRing`: virtual-node ring, add/remove workers |
| `tensor-rag/src/main.rs` | **MODIFY** | Mode dispatch (gateway/worker), gateway HTTP server, worker inner server, env var parsing |
| `tensor-rag/src/mcp.rs` | **MODIFY** | Two-tier session cache (local + Redis), `get_session` async method, concurrent handling |
| `tensor-rag/src/metrics.rs` | **MODIFY** | Add gateway metrics (upstream latency, routing errors, worker health gauges) |
| `tensor-rag/src/lib.rs` | **MODIFY** | Add `pub mod flight`, `pub mod session_store`, `pub mod consistent_hash` |
| `tensor-rag/Cargo.toml` | **MODIFY** | Add `arrow-flight`, `redis`, `reqwest` (dev for bench), `fastrand` (dev for bench), `tokio-tungstenite` (optional WS) |
| `tensor-rag/benches/stress_test.rs` | **CREATE** | 50k req/s stress test harness with multiple modes |
| `UNFINISHED_TASKS_AND_PLAN.md` | **MODIFY** | Mark Phase C progress (4.3.1–4.3.4) |

---

## 6. Dependencies Added

| Crate | Version | Reason |
|-------|---------|--------|
| `arrow-flight` | `51.0.0` | Arrow Flight gRPC service for cross-instance RecordBatch exchange |
| `redis` | `0.27` | Distributed session store + result cache |
| `reqwest` | `0.12` (dev only) | HTTP client for gateway proxying + stress test |
| `fastrand` | `2` (dev only) | Fast random session ID selection in stress test |
| `tokio-tungstenite` | `0.24` (optional) | WebSocket proxy for gateway |

---

## 7. Testing Strategy

### Unit Tests

| Test | Location | What It Covers |
|------|----------|----------------|
| `consistent_hash::test_ring_basic` | `consistent_hash.rs` | 3 workers, hash 1000 keys, verify distribution |
| `consistent_hash::test_ring_sticky` | `consistent_hash.rs` | Same key → same worker every time |
| `consistent_hash::test_ring_add_worker` | `consistent_hash.rs` | Adding worker only remaps ~1/N of keys |
| `session_store::test_redis_roundtrip` | `session_store.rs` | Save → Load → Delete (requires `redis-server`) |
| `session_store::test_session_ttl` | `session_store.rs` | Session expires after TTL (mocked time) |
| `session_store::test_cache_hit_miss` | `session_store.rs` | Result cache hit returns bytes, miss returns None |
| `flight::test_ticket_parse` | `flight.rs` | Ticket string parsing for all prefixes |
| `flight::test_shm_roundtrip` | `flight.rs` | Write batch to shm, read via Flight do_get |
| `mcp::test_get_session_local_cache` | `mcp.rs` | Local cache hit without Redis |
| `mcp::test_get_session_redis_fallback` | `mcp.rs` | Cache miss → Redis load → local cache populate |

### Integration Tests

| Test | Location | What It Covers |
|------|----------|----------------|
| `gateway_integration::test_gateway_routes` | `lib.rs` | Gateway → 1 worker, verify response |
| `gateway_integration::test_gateway_consistent_hash` | `lib.rs` | Same session → same worker across 10 requests |
| `gateway_integration::test_gateway_worker_failure` | `lib.rs` | One worker goes down, gateway routes to remaining |
| `gateway_integration::test_pipeline_via_gateway` | `lib.rs` | Full E2E: Gateway → Worker → Planner → VM → Response |
| `gateway_integration::test_session_persistence` | `lib.rs` | Worker restart, session recovered from Redis |
| `gateway_integration::test_flight_cross_worker` | `lib.rs` | Worker A requests data from Worker B via Flight |
| `stress_test` | `benches/stress_test.rs` | 50k req/s target |

### How to Run

```bash
# Unit tests (no services needed)
cargo test -p tensor-rag --lib consistent_hash::
cargo test -p tensor-rag --lib mcp::
cargo test -p tensor-rag --lib flight::

# Redis-dependent tests
REDIS_URL=redis://127.0.0.1:6379 cargo test -p tensor-rag --lib session_store::

# Integration tests (gateway + min 1 worker required)
# Terminal 1: WORKER_ID=0 MCP_PORT=9091 cargo run -- worker
# Terminal 2: GATEWAY_WORKERS="http://127.0.0.1:9091" cargo run -- gateway
# Terminal 3:
GATEWAY_URL=http://127.0.0.1:8080 cargo test -p tensor-rag --test gateway_integration -- --test-threads=1

# Stress test (full cluster)
cargo run --release --bench stress_test
```

---

## 8. Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| **Arrow Flight adds latency** (serialization + gRPC overhead) | Cross-worker data exchange slower than in-process `Arc` | Flight only used for cross-worker data; same-worker data still uses in-process `Arc`. Add latency benchmark gate in CI. |
| **Consistent hash + sticky sessions cause imbalance** | Hot sessions (high QPS) overload one worker | Use virtual nodes (100+ per worker) for better distribution. Add per-worker load metrics exposed in Prometheus. Future: weighted consistent hashing based on actual load. |
| **Redis becomes SPOF** | Session loss on Redis outage | Local cache as fallback (best-effort). Session state is small and only tracks `expected_sequence`. A Redis outage means new sessions only (existing sessions with local cache continue). |
| **Gateway single point of failure** | Gateway crash = total outage | Gateway is stateless — can run multiple instances behind a TCP/HTTP load balancer (e.g., nginx, HAProxy, or k8s Service). Each gateway independently computes consistent hash. |
| **50k req/s not achievable** | Misses target | Baseline single-node throughput first, then add workers. If gateway becomes bottleneck, horizontally scale gateway instances. Use connection pooling (keep-alive) between gateway and workers. |
| **WebSocket statefulness complicates routing** | WS connection pinned to one gateway instance | WS connections are sticky to gateway instance at the L7 LB layer (e.g., nginx `ip_hash`). Gateway proxies to same worker via consistent hash of `session_id`. |
| **Shared memory naming collisions across workers** | Data corruption | Scope shared memory names with `WORKER_ID` prefix: `tensor_rag_shm_{worker_id}_{region_name}`. |
| **Concurrent session access on worker** | Race conditions | Each worker handles one request per session at a time (sequence number ordering enforces this). `McpServer` is behind an `Arc<Mutex<>>` for the inner HTTP handler. |

---

## 9. Performance Budget

### Latency Budget (per request)

| Stage | Budget | Notes |
|-------|--------|-------|
| Gateway routing + hashing | 50 µs | Consistent hash + reqwest proxy, no serialization |
| Worker accept + parse | 50 µs | Axum JSON deserialization |
| Session lookup | 10 µs | Local HashMap (hot) / 500 µs Redis (cold) |
| Planner::compile | 5 µs | Pure AST → Program |
| DbPools::search_qdrant | 2 ms | Network I/O (gRPC to Qdrant) |
| VM execution | 1 ms | 10-50 ops on Arrow data |
| Serialization | 100 µs | FlatBuffer encode |
| **Total** | **~3.2 ms** | Within 5 ms P99 target |

### Memory Budget

| Component | Per 10k Sessions | Notes |
|-----------|------------------|-------|
| Gateway | ~10 MB | Stateless, just routes |
| Worker process | ~100 MB | VM, batch, cache per worker |
| Redis session store | ~1 MB | ~100 bytes/session |
| Shared memory | ~200 MB | `RecordBatch` temporary buffers |
| **Total** | **~311 MB** | Under 500 MB target |

---

## 10. Future Extensions (Post-Phase C)

- **Auto-scaling:** Worker discovery via Redis pub/sub or etcd. Workers register themselves, gateway watches registry and updates consistent hash ring dynamically.
- **Result deduplication:** Multiple workers may compute the same query (e.g., cache miss → all workers recompute). Use Redis result cache to return identical FlatBuffer data.
- **Distributed rate limiting:** Token bucket per session stored in Redis (atomic `DECR` + TTL). Central rate limit enforced at gateway.
- **ColBERT token sharding:** Distribute ColBERT token embeddings across workers by doc ID hash. Cross-worker MaxSIM via Arrow Flight.
- **TLS/mTLS:** Add optional TLS to gateway → worker and worker ↔ Flight connections. Cert rotation support.
- **gRPC MCP transport:** In addition to HTTP/WS, accept MCP requests over gRPC (native MCP protocol). `tonic` already a dependency.

---

## 11. Verification Checklist

- [ ] 4.3.1 — Arrow Flight service compiles, `do_get` returns `RecordBatch` for `qdrant:` and `shm:` tickets
- [ ] 4.3.1 — Flight `do_put` writes incoming batch to shared memory for local consumption
- [ ] 4.3.1 — Cross-worker Flight test: Worker A reads Qdrant data via Worker B's Flight service
- [ ] 4.3.2 — Gateway starts on `GATEWAY_PORT`, proxies `/api/mcp` to correct worker
- [ ] 4.3.2 — Worker inner HTTP server accepts forwarded MCP requests and returns FlatBuffer response
- [ ] 4.3.2 — WebSocket proxy establishes bidirectional communication
- [ ] 4.3.3 — Consistent hash: same `session_id` always maps to same worker
- [ ] 4.3.3 — Adding/removing 1 worker remaps ≤ 1/N of sessions
- [ ] 4.3.3 — Two-tier session cache: local hit in microseconds, Redis fallback works
- [ ] 4.3.3 — Worker restart + Redis recovery: session survives with correct `expected_sequence`
- [ ] 4.3.4 — Stress test achieves ≥50k req/s sustained for 10s with <0.1% error rate
- [ ] 4.3.4 — Baseline single-node throughput documented (for comparison)
- [ ] Regression: Single-node in-process path still works identically with no extra deps
- [ ] Regression: All existing unit + integration tests pass
- [ ] Performance: P99 latency with gateway + 1 worker < 5.5ms (0.5ms gateway overhead)
