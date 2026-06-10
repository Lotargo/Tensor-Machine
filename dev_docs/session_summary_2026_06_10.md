# Session Summary: 2026-06-10

## Что было сделано

### 1. Пространственное представление сущностей (задача #7)

Реализована система сущностей с эмбеддингами и пространственным представлением:

| Компонент | Описание |
|---|---|
| `db/entities.rs` | CRUD: upsert, link_chunk_entities, embed_and_store, update_layout, delete |
| `db/pool.rs` | SQL миграции: `entities`, `chunk_entities`; Qdrant коллекция `entities`; методы-обёртки |
| `db/search.rs` | `search_entity_vector_candidates` — векторный поиск через Qdrant + link на chunks |
| `db/document.rs` | Очистка entities при delete_document и clear_documents |
| `config.rs` | `get_qdrant_entities_collection()` |
| `router.rs` | При индексации: дедупликация → upsert → link → embed → Qdrant |
| `mcp/executor.rs` | RetrieveEntityScores: гибридный (векторный + текстовый) поиск; RetrieveQdrant: merged entity candidates |

**Schema БД:**
```sql
entities (id, doc_uuid, text, cluster_id, cluster_distance, layout_hint, UNIQUE(doc_uuid, text))
chunk_entities (chunk_id, entity_id)  -- junction table, CASCADE delete
```

### 2. Таймауты в add_qdrant_node (исправление зависаний)

Добавлен `tokio::time::timeout(10s)` на все вызовы `collection_info` и `create_collection` в `pool.rs`. Ранее вызовы блокировались навсегда при недоступном Qdrant.

### 3. Разделение тестов (integration feature)

- `Cargo.toml`: feature `integration` (default = [])
- `tests/qdrant_debug.rs`, `tests/graph_test.rs`, `tests/flight_integration.rs`: `#![cfg(feature = "integration")]`
- `cargo test` — только unit тесты (70 + 2), не зависает
- `cargo test --features integration` — все тесты

### 4. Парсер DSML-токенов (agent.rs)

Добавлена функция `parse_dsml_tool_calls` для извлечения tool calls из DSML-формата DeepSeek моделей:
```
<｜DSML｜invoke name="search_text">
<｜DSML｜parameter name="query" string="true">значение</｜DSML｜parameter>
</｜DSML｜invoke>
```

Конвертирует в стандартный OpenAI формат `tool_calls`. Работает в обоих режимах (streaming/non-streaming).

### 5. Новый баг #8: Фильтрация по chapter_id не возвращает фрагменты

Задокументирован в `bug_report_search_issues.md`. Проблема: агент корректно вызывает search_text с filters по chapter_id, но результат пустой. Протестировано на нескольких моделях.

---

## Ключевые файлы (изменённые в сессии)

| Файл | Изменения |
|---|---|
| `config.rs` | `get_qdrant_entities_collection()` |
| `db/entities.rs` | **Новый** — CRUD сущностей |
| `db/mod.rs` | Добавлен модуль `entities` |
| `db/pool.rs` | SQL миграции, Qdrant entities collection, методы, таймауты |
| `db/search.rs` | `search_entity_vector_candidates` |
| `db/document.rs` | Очистка entities, TRUNCATE с entities/chunk_entities |
| `ai/agent.rs` | DSML парсер, `mut message`, inject tool_calls |
| `mcp/executor.rs` | Гибридный entity search, merged entity candidates |
| `router.rs` | Entity pipeline при индексации |
| `Cargo.toml` | Feature `integration` |
| `tests/*.rs` | `#![cfg(feature = "integration")]` для интеграционных тестов |

---

## БД (текущее состояние)

- PostgreSQL в Docker: `tensor_postgres`, порт 5432
- Qdrant в Docker: `tensor_qdrant`, порт 6333-6334
- Один документ: `Утро_ч.1_в.1.docx` (doc_uuid: `73ebd244-f30c-447f-9784-0096c2c1f2b3`)
- 3971 чанк, 86 anchors, 0 edges
- Новые таблицы: `entities`, `chunk_entities` (автоматически при запуске)

---

## Открытые задачи

1. **#6** — BM25-кандидаты не фильтруются в гибридном поиске
2. **#8** — Фильтрация по chapter_id не возвращает фрагменты главы (диагностика + исправление)
