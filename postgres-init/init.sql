CREATE TABLE IF NOT EXISTS documents (
    id BIGSERIAL PRIMARY KEY,
    text TEXT NOT NULL,
    bm25_score REAL NOT NULL DEFAULT 0.0
);

CREATE TABLE IF NOT EXISTS anchors (
    id BIGSERIAL PRIMARY KEY,
    doc_uuid TEXT NOT NULL,
    name TEXT NOT NULL,
    text TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_anchors_doc_uuid ON anchors(doc_uuid);
CREATE INDEX IF NOT EXISTS idx_anchors_name ON anchors(doc_uuid, name);

CREATE TABLE IF NOT EXISTS edges (
    id BIGSERIAL PRIMARY KEY,
    source_doc_uuid TEXT NOT NULL,
    target_doc_uuid TEXT NOT NULL,
    edge_type TEXT NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source_doc_uuid);
CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target_doc_uuid);
CREATE INDEX IF NOT EXISTS idx_edges_type ON edges(edge_type);

-- Seed with some sample documents
INSERT INTO documents (id, text, bm25_score) VALUES
(1, 'Tensor Machine is a high-performance vector search engine built in Rust.', 4.5),
(2, 'Rust is a systems programming language focused on safety, speed, and concurrency.', 3.2),
(3, 'Arrow Flight enables high-speed data transfer between distributed vector VM nodes.', 5.1)
ON CONFLICT (id) DO NOTHING;

-- Reset sequence generator to prevent conflict on next insert
SELECT setval('documents_id_seq', COALESCE((SELECT MAX(id)+1 FROM documents), 1), false);
