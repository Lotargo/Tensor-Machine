# Bug Report: Проблемы с поиском по главам и фильтрацией

**Дата:** 2026-06-10  
**Статус:** Обнаружены, требуют исправления

---

## Суть проблемы

При запросе "расскажи что было в 27 главе и что этому предшествовало?" агент вызывает `search_text` с query "двадцать седьмая глава" — и не может найти ничего конкретного.

**Корень проблемы:** Агент пытается "угадать" chapter_id вместо того, чтобы использовать реальные названия заголовков из документа.

---

## Почему костыли с распознаванием чисел — плохая идея

Документ может содержать заголовки любого формата:
- "2.3.1 Подраздел"
- "Вступление"  
- "Глава 27. Кризис"
- "Раздел III. Результаты"
- "Эпилог"

Мы не можем заранее предусмотреть все варианты. **Нужно использовать те названия, которые уже извлечены из документа.**

---

## Что уже есть в системе

### 1. Anchors (заголовки)
В БД уже хранятся точные названия заголовков 1:1 как в оригинале:

```sql
-- Таблица anchors
SELECT name, text FROM anchors WHERE doc_uuid = '...';
-- name: "h1-1", "h2-3", "heading-5"
-- text: "Глава 27. Кризис", "Вступление", "2.3.1 Подраздел"
```

### 2. Инструмент get_document_metadata
Возвращает anchors вместе с другими метаданными документа:

```json
{
  "doc_uuid": "...",
  "filename": "...",
  "metadata": { ... },
  "anchors": [
    { "name": "h1-1", "text": "Глава 1. Начало" },
    { "name": "h1-2", "text": "Глава 2. Развитие" },
    { "name": "h1-27", "text": "Глава 27. Кризис" },
    { "name": "h1-41", "text": "Глава 41. Финал" }
  ],
  "edges": [...]
}
```

### 3. Фильтры в search_text
Поддерживаются фильтры по полям documents:
```json
{
  "query": "...",
  "filters": [
    { "field": "chapter_id", "operator": "==", "value": "27" }
  ]
}
```

---

## Проблема: Агент не использует эту возможность

### Причина 1: Фильтры не объявлены в tool_definitions
В `agent.rs:331-415` параметры `filters` и `metadata_filters` отсутствуют:

```rust
fn tool_definitions() -> Vec<serde_json::Value> {
    vec![
        json!({
            "name": "search_text",
            "parameters": {
                "properties": {
                    "query": { ... },
                    "final_top_k": { ... },
                    "doc_uuid": { ... }
                    // filters — ОТСУТСТВУЕТ
                    // metadata_filters — ОТСУТСТВУЕТ
                }
            }
        }),
    ]
}
```

### Причина 2: Нет инструкции использовать anchors
Агент не знает, что нужно:
1. Сначала вызвать `get_document_metadata`
2. Найти нужный anchor по текстовому запросу
3. Использовать его name для фильтрации

---

## Решение: Научить агента работать с anchors

### Шаг 1: Расширить tool_definitions
Добавить фильтры в schema инструмента `search_text`:

```rust
json!({
    "name": "search_text",
    "description": "Search the local RAG document library. For chapter/section searches, first call get_document_metadata to get exact anchor names, then use filters.",
    "parameters": {
        "properties": {
            "query": { 
                "type": "string",
                "description": "Search query text"
            },
            "final_top_k": { ... },
            "doc_uuid": { ... },
            "filters": {
                "type": "array",
                "description": "Metadata filters. For chapter search: use exact anchor names from get_document_metadata.",
                "items": {
                    "type": "object",
                    "properties": {
                        "field": { 
                            "type": "string",
                            "enum": ["chapter_id", "doc_uuid", "chunk_level", "chunk_index"]
                        },
                        "operator": { 
                            "type": "string",
                            "enum": ["==", "!=", ">", ">=", "<", "<=", "in"]
                        },
                        "value": {}
                    },
                    "required": ["field", "operator", "value"]
                }
            }
        },
        "required": ["query"]
    }
})
```

### Шаг 2: Добавить инструкцию в промпт агента
В system prompt добавить явную инструкцию:

```
Для поиска по главам/разделам:
1. Сначала вызови get_document_metadata для получения списка anchors
2. Найди нужный anchor по текстовому запросу пользователя
3. Используй точное name anchor в фильтре search_text

Пример:
- Пользователь: "расскажи что было в 27 главе"
- Шаг 1: get_document_metadata(doc_uuid="...") → anchors: [{"name": "h1-27", "text": "Глава 27. Кризис"}, ...]
- Шаг 2: search_text(query="...", filters=[{"field": "chapter_id", "operator": "==", "value": "27"}])
```

### Шаг 3: Оптимизация — поиск anchors по query
Добавить в `get_document_metadata` опциональный параметр `query` для предварительной фильтрации anchors:

```rust
// В tool_definitions
"get_document_metadata": {
    "parameters": {
        "properties": {
            "doc_uuid": { ... },
            "query": {
                "type": "string",
                "description": "Optional: filter anchors by text similarity"
            }
        }
    }
}
```

---

## Альтернативное решение: Автоматический поиск anchor по query

Если не хочется нагружать агента двумя вызовами, можно добавить в `search_text` логику:

1. Принять `query` от агента
2. Найти наиболее релевантный anchor через семантический поиск в Qdrant
3. Автоматически добавить фильтр по `chapter_id`

```rust
// В search_text.rs перед выполнением поиска
if let Some(anchor_name) = find_relevant_anchor(pools, &query, &doc_uuid).await {
    // Добавить фильтр автоматически
    filters.push(Filter {
        field: "chapter_id".to_string(),
        operator: "==".to_string(),
        value: extract_chapter_number(&anchor_name),
    });
}
```

---

## Сравнение подходов

| Подход | Плюсы | Минусы |
|--------|-------|--------|
| **Расширение parse_text_number** | Просто в реализации | Не масштабируется, хрупко |
| **Научить агента использовать anchors** | Гибко, точно, масштабируется | Требует 2 вызова инструмента |
| **Авто-поиск anchor в search_text** | Прозрачно для агента | Скрытая логика, сложнее отладка |

**Рекомендация:** Комбинация — расширить `tool_definitions` + добавить инструкцию в промпт. Авто-поиск как fallback.

---

## Тестовые сценарии

### Сценарий 1: Глава по номеру
**Запрос:** "расскажи что было в 27 главе"  
**Ожидаемый workflow:**
1. `get_document_metadata(doc_uuid="...")` → anchors с "Глава 27. Кризис"
2. `search_text(query="...", doc_uuid="...", filters=[{"field": "chapter_id", "operator": "==", "value": "27"}])`

### Сценарий 2: Глава по названию
**Запрос:** "что происходит во вступлении"  
**Ожидаемый workflow:**
1. `get_document_metadata(doc_uuid="...")` → anchors с "Вступление"
2. `search_text(query="...", doc_uuid="...", filters=[{"field": "chapter_id", "operator": "==", "value": "вступление"}])`

### Сценарий 3: Подраздел
**Запрос:** "объясни раздел 2.3.1"  
**Ожидаемый workflow:**
1. `get_document_metadata(doc_uuid="...")` → anchors с "2.3.1 Подраздел"
2. `search_text(query="...", doc_uuid="...", filters=[{"field": "chapter_id", "operator": "==", "value": "2.3.1"}])`

---

## Связанные файлы

| Файл | Что нужно изменить |
|------|-------------------|
| `tensor-rag/src/ai/agent.rs:331-415` | Добавить `filters` в `tool_definitions` |
| `tensor-rag/src/ai/agent.rs` | Обновить system prompt с инструкцией по anchors |
| `tensor-rag/src/mcp/tools/get_document_metadata.rs` | Опциональный параметр `query` для фильтрации anchors |
| `tensor-rag/src/db/helpers.rs` | Убрать `parse_text_number` (не нужен) |
| `tensor-rag/src/ir.rs:8-26` | `QueryRequest.filters` — уже есть, просто не используется |

---

## Итог

**Не нужно:**
- Распознавать числа прописью
- Писать regex для всех форматов заголовков
- Хардкодить паттерны

**Нужно:**
- Расширить `tool_definitions` — добавить `filters`
- Научить агента использовать `get_document_metadata` → anchors
- Использовать точные названия из документа

---

## Дополнительные находки при анализе кода

При детальном изучении кодовой базы были обнаружены следующие критические проблемы, которые необходимо учесть при планировании исправлений:

### 1. Ограничение `FilterByMetadata` в VM
Инструкция `FilterByMetadata` в [executor.rs](file:///f:/projects/APL-RAG/tensor-rag/src/mcp/executor.rs) запрашивает из БД только `id` и `metadata`:
`SELECT id, metadata FROM documents WHERE id = ANY($1)`
Затем она пытается найти поля фильтрации (например, `chapter_id`, `chunk_level`, `doc_uuid`, `chunk_index`) в JSON-поле `metadata`. Однако эти системные поля хранятся как отдельные физические колонки в таблице `documents` и отсутствуют в JSON-объекте `metadata`. Из-за этого любой фильтр по `chapter_id` или другим системным полям на уровне VM всегда отбрасывает 100% результатов.
**Решение:** Научить `FilterByMetadata` запрашивать и проверять системные колонки БД наряду с JSONB-полем `metadata`.

### 2. Обход фильтрации BM25/keyword кандидатов в гибридном поиске
Параметр `filters` из `QueryRequest` передается только в `RetrieveQdrant`. При гибридном поиске кандидаты, полученные через лексический/сущностный поиск из Postgres, не фильтруются на уровне VM (так как `filters` не компилируется в инструкцию `FilterByMetadata` — для этого нужен `metadata_filters`). В результате гибридный поиск может возвращать фрагменты из посторонних документов/глав, просочившиеся через BM25.
**Решение:** При наличии `filters` также применять фильтрацию на уровне VM ко всем объединенным кандидатам, либо транслировать `filters` в SQL-запрос лексического поиска.

### 3. Искажение кодировки регулярного выражения в `db/helpers.rs`
В [db/helpers.rs](file:///f:/projects/APL-RAG/tensor-rag/src/db/helpers.rs) на строке 121 регулярное выражение для извлечения глав содержит битые UTF-8 последовательности вместо кириллицы:
`r"(?i)(?:Chapter|Part|Р“Р»Р°РІР°|Р§Р°СЃС‚СЊ)\s+..."`
**Решение:** Восстановить корректные русские слова `Глава` и `Часть`.

### 4. Несовпадение имени поля Anchors в `get_document_metadata`
В `get_document_metadata` возвращается поле `"text_preview"`, а не `"text"`, как изначально предполагалось в данном отчете. Необходимо проинструктировать агента использовать именно `"text_preview"`.
