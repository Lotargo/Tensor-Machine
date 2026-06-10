# Export Script: doc_details_current.json

Скрипт для экспорта документа из БД в JSON-формат, идентичный формату API `/documents/:doc_uuid/details`.

## Файл

`F:\projects\APL-RAG\export_doc.py`

## Зависимости

```bash
pip install psycopg2-binary
```

## Использование

```bash
python F:\projects\APL-RAG\export_doc.py
```

## Конфигурация в скрипте

```python
DOC_UUID = "73ebd244-f30c-447f-9784-0096c2c1f2b3"  # UUID документа
OUTPUT = "F:/projects/APL-RAG/doc_details_current.json"  # Выходной файл
```

Подключение к БД:
```python
conn = psycopg2.connect(
    host="localhost",
    port=5432,
    dbname="tensor_rag",
    user="postgres",
    password="postgres",
)
```

## Формат выходного JSON

```json
{
  "doc_uuid": "...",
  "anchors": [{"name": "h1-1", "text": "Глава 1"}],
  "chunks": [{
    "id": 1,
    "text": "...",
    "chunk_index": 0,
    "chunk_level": "macro",
    "parent_chunk_index": null,
    "macro_index": null,
    "medium_index": null,
    "header_chain": [],
    "zone_keywords": ["..."],
    "entities": ["..."],
    "cluster_id": 3,
    "cluster_distance": 0.23,
    "layout_hint": [x, y, z]
  }],
  "edges": [],
  "semantic_clusters": [],
  "semantic_clusters_loaded": false
}
```

## Важные замечания

- PowerShell отображает кириллицу в консоли некорректно (CP866 vs UTF-8), но файл пишется правильно
- Python `json.dump` с `ensure_ascii=False` + `indent=2` — корректная кодировка и форматирование
- `docker exec tensor_postgres psql` — альтернатива для запросов, но кириллица ломается в выводе
- API endpoint `/documents/:doc_uuid/details` — не запущен в текущей сессии (сервер не работает)
