# Отчет по внедрению porting_from_rag3

Дата: 2026-06-04

## Выполнено

- Добавлен трёхуровневый hierarchical chunking в `tensor-rag/src/chunker.rs`: `Macro`, `Medium`, `Micro`, `TextSpan`, token-aware overlap, header-aware boundaries, semantic zoning, header chain, chapter id и parent-связи.
- Upload flow в `tensor-rag/src/router.rs` переведен на `HierarchicalChunker`; embeddings строятся по enriched text: file, level, zone, header context и текст чанка.
- Расширена PostgreSQL/Qdrant схема в `tensor-rag/src/db.rs`: `chunk_level`, `parent_chunk_index`, `macro_index`, `medium_index`, `header_chain`, `zone_keywords`, `entities`, индексы для hierarchy и GIN по entities.
- Добавлен entity extraction/search light в `tensor-rag/src/entity.rs`: named entities, query entities, merge по light stemming, entity candidate expansion и entity score channel.
- Добавлен normalization pipeline в `tensor-rag/src/normalizer.rs`: hyphen join, whitespace collapse, unicode tokenization, RU/EN stopwords, light stemming и top terms.
- Добавлен RSF в `tensor-rag/src/rsf.rs`, IR/DSL/Planner/VM: dense + BM25 + entity channels, min-max normalization, configurable weights.
- Исправлены заглушки MMR/RRF в `tensor-rag/src/vm.rs`: MMR теперь использует формулу `lambda * relevance - (1-lambda) * max doc similarity`, RRF фьюзит реальные ranked lists.
- `search_text` теперь делает parent resolution: micro/medium результаты поднимаются к macro/parent контексту, с fallback на child text.
- Добавлены response helpers в `tensor-rag/src/response.rs`: deduplication, bucket-like ordering helper, query history pruning.
- Добавлен clustering fallback module в `tensor-rag/src/clustering.rs`: trait-структура `DimensionReducer`/`Clusterer`, PCA-like reducer, radius clusterer, diversity bonus. Полноценные UMAP/HDBSCAN не подключались внешними зависимостями в этом проходе.
- Добавлены focused tests для hierarchical chunker, entity extraction, normalizer и RSF planner.

## Частично выполнено

- UMAP/HDBSCAN реализован как безопасная архитектурная заготовка с fallback-кластеризацией, без внешних crates `umap-rs`/`hdbscan-rs`.
- Summary injection и полноценный response builder из RAG3 не перенесены целиком; реализованы dedup/context helpers и parent-resolution форматирование.
- Stopword fallback для Qdrant/BM25 как отдельный retry-механизм не потребовался: текущий BM25 канал уже возвращает `0.0` scores при пустом clean query, но отдельный error-based retry не добавлялся.

## Проверка

- Выполнено `cargo fmt`.
- Выполнено `cargo test`.
- Результат: все тесты прошли, включая unit/integration/doc-test группы. Основной прогон показал `42 passed` в unit tests и успешные integration tests `chunk_test`, `flight_integration`, `qdrant_debug`.

## Примечания

- `cargo fmt` отформатировал весь Rust crate, поэтому diff включает форматирование файлов, которые не были логически изменены в рамках миграции.
- Исходный документ `docs/porting_from_rag3.md` не изменялся.
