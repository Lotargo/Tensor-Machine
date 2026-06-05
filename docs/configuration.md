# Configuration

## Environment Variables

The server reads configuration from environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_URL` | — | PostgreSQL connection string |
| `QDRANT_URL` | — | Qdrant gRPC endpoint |
| `QDRANT_COLLECTION` | — | Qdrant collection name |
| `MCP_BIND_ADDR` | `0.0.0.0:8080` | HTTP/MCP/router listen address. When unset and the default port is busy, the server tries fallback ports `8081..8090`. |
| `PORTAL_HOST` | `127.0.0.1` | Hostname printed for the local portal URL when the router binds to `0.0.0.0` or `::`. |
| `MAX_CONNECTIONS` | `5` | PostgreSQL connection pool size |

When no database variables are set, the system runs in **mock mode** and returns synthetic data for all queries.

## Database Setup

### PostgreSQL

Stores BM25 scores and document metadata.

**Schema:**

```sql
CREATE TABLE documents (
    id BIGSERIAL PRIMARY KEY,
    text TEXT NOT NULL,
    bm25_score REAL NOT NULL DEFAULT 0.0
);
```

**Connection:**

```bash
export DATABASE_URL="postgres://user:password@localhost:5432/tensor_rag"
```

### Qdrant

Stores vector embeddings. Configure via the Qdrant URI and collection name.

```bash
export QDRANT_URL="http://localhost:6334"
export QDRANT_COLLECTION="documents"
```

**Expected vector configuration:**

- Dimension: 768 (configurable via Arrow schema)
- Distance: Dot product or cosine similarity

### Mock Mode

When no database is configured, `DbPools` returns synthetic data:

- Qdrant: Random vectors with sequential IDs
- PostgreSQL: Zero BM25 scores with placeholder text

This enables full pipeline testing without external infrastructure.

## Programmatic Configuration

Configure connection pools directly in Rust code:

```rust
use tensor_rag::db::DbPools;

let mut pools = DbPools::new();

// PostgreSQL
pools.init_postgres("postgres://user:pass@localhost/db").await?;

// Qdrant nodes
pools.add_qdrant_node("default", "http://localhost:6334").await?;

// ML servers (optional — for embedding/reranking models)
pools.add_ml_server("embedding", "http://embedding-service:50051").await?;
```

## VM Configuration

| Parameter | Default | Description |
|-----------|---------|-------------|
| Max instructions | 100,000 | Maximum VM instruction count per query (prevents runaway execution) |

## Arrow Schema

The default RecordBatch schema used across all subsystems:

| Column | Arrow Type | Description |
|--------|------------|-------------|
| `id` | `Int64` | Document ID |
| `vector` | `FixedSizeList(Float32, 768)` | 768-dim embedding vector |
| `bm25_score` | `Float32` | Pre-computed BM25 score |
| `text` | `Utf8` | Document text |

## Performance Targets

| Metric | Target | Measured |
|--------|--------|----------|
| P99 latency (Top-1000, VM only) | < 5 ms | ~50 µs in CI |
| Memory per 10k MCP sessions | < 500 MB | ~0.01 MB/session |
| Throughput | 50,000 req/s | — |
