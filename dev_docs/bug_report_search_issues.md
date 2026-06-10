# Bug Report: Проблемы с поиском по главам и фильтрацией

**Дата:** 2026-06-10  
**Обновлено:** 2026-06-10 (проверка текущего состояния)

---

## Справочные материалы

- **Экспорт документа из БД:** `dev_docs/export_script_reference.md` — скрипт `export_doc.py` для получения JSON в формате API `/documents/:doc_uuid/details`
- **Текущий экспортированный документ:** `doc_details_current.json` — документ `Утро_ч.1_в.1.docx` (doc_uuid: `73ebd244-f30c-447f-9784-0096c2c1f2b3`, 3971 чанк, 86 anchors)

---

## Статусы

- `open` — обнаружен, не исправлен
- `fixed` — исправлен и проверен
- `partial` — частично исправлен

---

## Исправленные пункты

### 1. Фильтры отсутствовали в tool_definitions — `fixed`

В `agent.rs:331-415` параметр `filters` **уже добавлен** в schema инструмента `search_text`:

```rust
"filters": {
    "type": "array",
    "description": "Metadata filters to apply to search. For chapter search: first get metadata to inspect exact chapter_id values, then pass a filter.",
    "items": {
        "type": "object",
        "properties": {
            "field": { "type": "string", "enum": ["chapter_id", "doc_uuid", "chunk_level", "chunk_index"] },
            "operator": { "type": "string", "enum": ["==", "!=", ">", ">=", "<", "<=", "in"] },
            "value": {}
        },
        "required": ["field", "operator", "value"]
    }
}
```

**Файл:** `tensor-rag/src/ai/agent.rs:353-371`

---

### 2. Не было инструкции по использованию anchors — `fixed`

В `context.rs:146-150` добавлена явная инструкция:

```
For questions asking about a specific chapter, section, or part (e.g., "what happened in chapter 27?"):
1. Call get_document_metadata for the document to get the list of anchors (each anchor has "name" and "text_preview").
2. Find the anchor matching the requested chapter/section text (e.g., "Глава 27" or "2.3").
3. Use the matching chapter number/identifier to construct a filter for search_text.
4. Call search_text with filters parameter (e.g., filters=[{"field": "chapter_id", "operator": "==", "value": "27"}]).
```

**Файл:** `tensor-rag/src/ai/context.rs:146-150`

---

### 3. Ограничение `FilterByMetadata` в VM — `fixed`

Текущий код в `executor.rs:586-704` корректно обрабатывает системные поля:

**SQL-запрос** (строка 604):
```sql
SELECT id, doc_uuid, chapter_id, chunk_index, chunk_level, metadata FROM documents WHERE id = ANY($1)
```

**Маршрутизация фильтров** (строки 631-641):
```rust
let val = match f.field.as_str() {
    "doc_uuid" => serde_json::Value::String(doc_uuid.clone()),
    "chapter_id" => serde_json::Value::String(chapter_id.clone()),
    "chunk_index" => serde_json::Value::Number(serde_json::Number::from(*chunk_index)),
    "chunk_level" => serde_json::Value::String(chunk_level.clone()),
    _ => match doc_metadata.get(&f.field) { ... }
};
```

**Файл:** `tensor-rag/src/mcp/executor.rs:586-704`

---

### 4. Искажение кодировки regex в `db/helpers.rs` — `fixed`

Регулярные выражения во всех 4 локациях содержат корректную UTF-8 кириллицу (`Глава|Часть`):

| Файл | Строка |
|---|---|
| `db/helpers.rs` | 121 |
| `mcp/executor.rs` | 10 |
| `chunker.rs` | 254 |
| `chunker.rs` | 810 |

Тест `extract_chapter_id_russian` проходит:
```
assert_eq!(extract_chapter_id("Глава 27"), Some("27".to_string()));
assert_eq!(extract_chapter_id("Часть вторая"), Some("2".to_string()));
```

---

### 5. Несовпадение имени поля Anchors — `fixed`

В `get_document_metadata` возвращается поле `"text_preview"` — это уже учтено в инструкции агента (пункт 2 выше).

---

## Открытые пункты

### 6. Обход фильтрации BM25/keyword кандидатов в гибридном поиске — `open`

Параметр `filters` из `QueryRequest` передается только в `RetrieveQdrant`. При гибридном поиске кандидаты из Postgres (BM25/keyword) не фильтруются на уровне VM. В результате гибридный поиск может возвращать фрагменты из посторонних документов/глав.

**Решение:** При наличии `filters` также применять фильтрацию на уровне VM ко всем объединенным кандидатам, либо транслировать `filters` в SQL-запрос лексического поиска.

**Файлы:** `tensor-rag/src/ir.rs:291-302`, `tensor-rag/src/mcp/executor.rs:586-704`

---

### 7. Сущности не имеют пространственного представления — `open`

Извлеченные сущности хранятся как простой JSON-массив текста в колонке `entities` таблицы `documents`. У них нет:

| Атрибут | Статус |
|---|---|
| Эмбеддинг | Отсутствует |
| 3D-позиция (layout_hint) | Отсутствует |
| Кластер (cluster_id) | Отсутствует |
| Связи между чанками | Только co-occurrence в `edges` |

**Текущая архитектура:**
```
documents表
├── entities: ["Арсений", "Мурадов", "Омега"]  ← просто текстовый массив
├── cluster_id: 3                               ← кластер ЧАНКА, не сущности
├── layout_hint: [x, y, z]                     ← позиция ЧАНКА, не сущности
└── cluster_distance: 0.23                      ← расстояние ЧАНКА, не сущности
```

**Проблемы:**
- Поиск сущностей работает только через текстовый matching (stemming + подстрока), требует минимум 2 совпадения
- В 3D-визуализации сущности не отображаются как отдельные узлы с позицией
- Нет граф-связей между сущностями из разных чанков
- Сущности из одного кластера чанков не связаны пространственно

**Решение:** Создать отдельную таблицу `entities` с:
- `id`, `doc_uuid`, `text` (уникальная сущность)
- `embedding` (векторный эмбеддинг)
- `layout_hint` (3D-координаты после UMAP)
- `cluster_id`, `cluster_distance` (результат HDBSCAN)
- Связь с чанками через промежуточную таблицу `chunk_entities`

**Файлы:**
- `tensor-rag/src/db/search.rs:1057-1115` — `search_entity_candidate_ids`
- `tensor-rag/src/mcp/executor.rs:327-360` — `RetrieveEntityScores`
- `postgres-init/init.sql` — schema

---

## Сравнение подходов (из исходного отчета)

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

| Файл | Статус |
|------|--------|
| `tensor-rag/src/ai/agent.rs:331-415` | `fixed` — filters в tool_definitions |
| `tensor-rag/src/ai/context.rs:146-150` | `fixed` — инструкция по anchors |
| `tensor-rag/src/mcp/executor.rs:586-704` | `fixed` — FilterByMetadata с системными колонками |
| `tensor-rag/src/db/helpers.rs:121` | `fixed` — корректная кодировка regex |
| `tensor-rag/src/db/search.rs:1057-1115` | `open` — нет spatial representation для сущностей |
| `postgres-init/init.sql` | `open` — нет таблицы entities |
