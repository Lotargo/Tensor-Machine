# Session Summary: 2026-06-10

## Что было сделано

### 1. Проверка багов из bug_report_search_issues.md

Проверены 5 пунктов из отчета. Все оказались **уже исправлены** в текущем коде:

| # | Проблема | Результат |
|---|---|---|
| 1 | Фильтры не в tool_definitions | `agent.rs:353-371` — filters добавлены |
| 2 | Нет инструкции по anchors | `context.rs:146-150` — инструкция добавлена |
| 3 | FilterByMetadata не видит системные колонки | `executor.rs:586-704` — SQL запрос корректен |
| 4 | Битая кодировка regex | `helpers.rs:121` — корректная UTF-8 кириллица |
| 5 | Несовпадение имени поля | Учтено в инструкции агента |

### 2. Экспорт документа из БД

- Сервер не запущен (API endpoint недоступен)
- Использован Python скрипт `export_doc.py` с psycopg2
- Файл `doc_details_current.json` создан (7.5 MB, 3971 чанк, 86 anchors)
- Формат идентичен `doc_details_after.json`

### 3. Анализ кластеризации

- `cluster_id=-1` (165 чанков, 4.1%) — шум HDBSCAN, это нормально
- `cluster_distance=0.0` для шумовых точек — корректно
- Остальные чанки имеют корректные кластеры и расстояния

### 4. Анализ сущностей

Выявлена недоработка: сущности хранятся как простой JSON-массив текста в колонке `entities`, без:
- Эмбеддинга
- 3D-позиции (layout_hint)
- Кластера (cluster_id)
- Граф-связей между чанками

Это пункт добавлен в bug_report как #7 (status: open).

---

## Ключевые файлы

| Файл | Назначение |
|---|---|
| `tensor-rag/src/ai/agent.rs:331-415` | tool_definitions (filters) |
| `tensor-rag/src/ai/context.rs:140-162` | Инструкции агента (anchors) |
| `tensor-rag/src/mcp/executor.rs:586-704` | FilterByMetadata реализация |
| `tensor-rag/src/db/helpers.rs:118-129` | extract_chapter_id regex |
| `tensor-rag/src/db/search.rs:1057-1115` | search_entity_candidate_ids |
| `tensor-rag/src/ir.rs:274-469` | Planner::compile (фильтры) |
| `tensor-rag/src/ai/mod.rs:16-32` | DEFAULT_SYSTEM_PROMPT |
| `postgres-init/init.sql` | Schema БД |
| `export_doc.py` | Экспорт документа в JSON |

---

## БД (текущее состояние)

- PostgreSQL в Docker: `tensor_postgres`, порт 5432
- Qdrant в Docker: `tensor_qdrant`, порт 6333-6334
- Ollama в Docker: `tensor_ollama`, порт 11434
- Один документ: `Утро_ч.1_в.1.docx` (doc_uuid: `73ebd244-f30c-447f-9784-0096c2c1f2b3`)
- 3971 чанк, 86 anchors, 0 edges

---

## Открытые задачи

1. **BM25-кандидаты не фильтруются** в гибридном поиске (пункт #6 в bug_report)
2. **Сущности без пространственного представления** (пункт #7 в bug_report) — нужна таблица `entities` с эмбеддингами, layout_hint, кластерами
