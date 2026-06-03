CREATE TABLE IF NOT EXISTS documents (
    id BIGSERIAL PRIMARY KEY,
    text TEXT NOT NULL,
    bm25_score REAL NOT NULL DEFAULT 0.0
);

-- Seed with some sample documents
INSERT INTO documents (id, text, bm25_score) VALUES
(1, 'Tensor Machine is a high-performance vector search engine built in Rust.', 4.5),
(2, 'Rust is a systems programming language focused on safety, speed, and concurrency.', 3.2),
(3, 'Arrow Flight enables high-speed data transfer between distributed vector VM nodes.', 5.1)
ON CONFLICT (id) DO NOTHING;

-- Reset sequence generator to prevent conflict on next insert
SELECT setval('documents_id_seq', COALESCE((SELECT MAX(id)+1 FROM documents), 1), false);
