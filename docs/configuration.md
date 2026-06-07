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
| `EMBEDDING_BATCH_SIZE` | `64` in `.env.example`, `32` code fallback | Number of chunks sent in one embedding request during ingestion. |
| `EMBEDDING_REQUEST_TIMEOUT_SECS` | `240` in `.env.example`, `120` code fallback | HTTP timeout for embedding batch requests. |
| `UPLOAD_MAX_BYTES` | `268435456` | Maximum accepted multipart upload size for `/upload` in bytes. |
| `QDRANT_UPSERT_WAIT` | `false` | Whether ingestion waits for Qdrant writes to be applied before continuing. |
| `QUERY_EXPANSION_MODE` | `off` | Query expansion mode. Supported values: `off`, `dictionary`. |
| `QUERY_EXPANSION_DICTIONARY_PATHS` | — | Semicolon- or comma-separated synonym dictionary files used when `QUERY_EXPANSION_MODE=dictionary`. |
| `QUERY_EXPANSION_MAX_TERMS` | `8` | Maximum extra lexical terms added to a query. |

When no database variables are set, the system runs in **mock mode** and returns synthetic data for all queries.

## Query Expansion

Query expansion is disabled by default. To enable dictionary-based expansion:

```bash
export QUERY_EXPANSION_MODE=dictionary
export QUERY_EXPANSION_DICTIONARY_PATHS="config/synonyms.json;config/domain-synonyms.txt"
export QUERY_EXPANSION_MAX_TERMS=8
```

Supported JSON dictionary formats:

```json
{
  "sing": ["chant", "vocalize"],
  "lullaby": ["cradle song"]
}
```

```json
[
  ["song", "tune", "melody"],
  ["lullaby", "cradle song"]
]
```

Text dictionaries use one synonym group per line:

```text
song, tune, melody
lullaby => cradle song
```

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

- Dimension: 1024 by default for `jeffh/intfloat-multilingual-e5-large-instruct:q8_0`
- Distance: Dot product or cosine similarity

Changing `EMBEDDING_MODEL_NAME`, `EMBEDDING_DIM`, or embedding prefixes requires recreating the Qdrant collection and reindexing documents so stored vectors match the active model.

The Docker Compose Ollama setup pulls `jeffh/intfloat-multilingual-e5-large-instruct:q8_0`. If you use a Hugging Face/Text Embeddings Inference/Together-style endpoint instead of Ollama, set `EMBEDDING_MODEL_NAME` to that provider's canonical model identifier, for example `intfloat/multilingual-e5-large-instruct`.

### Ollama Throughput

The compose service passes these runtime settings to Ollama:

```bash
OLLAMA_NUM_PARALLEL=4
OLLAMA_MAX_LOADED_MODELS=1
OLLAMA_MAX_QUEUE=1024
OLLAMA_KEEP_ALIVE=24h
OLLAMA_FLASH_ATTENTION=1
```

`OLLAMA_NUM_PARALLEL` allows concurrent work for the loaded model. `OLLAMA_MAX_LOADED_MODELS=1` is intentional for indexing: it keeps VRAM focused on the embedding model instead of allowing unrelated models to compete for memory. Running two copies of the same quantized embedding model is usually less useful than increasing batch size and pipelining requests, unless separate GPUs/endpoints are available and the client distributes batches across them.

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
| `vector` | `FixedSizeList(Float32, 1024)` | 1024-dim embedding vector |
| `bm25_score` | `Float32` | Pre-computed BM25 score |
| `text` | `Utf8` | Document text |

## Performance Targets

| Metric | Target | Measured |
|--------|--------|----------|
| P99 latency (Top-1000, VM only) | < 5 ms | ~50 µs in CI |
| Memory per 10k MCP sessions | < 500 MB | ~0.01 MB/session |
| Throughput | 50,000 req/s | — |
