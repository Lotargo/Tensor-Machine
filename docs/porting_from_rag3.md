# Porting Guide: `Private--test--RAG3/rag-service` → `tensor-rag`

> **Цель:** Перенести эффективные стратегии поиска, чанкования и пост-обработки из Go-прототипа
> в Rust-продакшен. Проблема текущего RAG — неэффективный поиск, отсутствие точного
> совпадения, сломанные гибридные стратегии.

---

## 1. Трёхуровневая иерархическая нарезка (Macro / Medium / Micro)

### Откуда: `rag3/rag-service/service/components/text_splitter.go` (адаптировать)

**Что есть в RAG3:**
- Parent (1024 токенов) + Child (256 токенов) — два уровня
- Child чанки ссылаются на Parent через `parent_chunk_index`

**Что требуется для tensor-rag: три уровня:**

| Уровень | Размер | Назначение | Индексация | Поиск |
|---------|--------|------------|------------|-------|
| **Macro** | ~2048 токенов (~8000 символов) | Контекст для LLM, глава/раздел целиком | Вектор + BM25 | Parent retrieval |
| **Medium** | ~512 токенов (~2000 символов) | Основная единица поиска, секция | Вектор + BM25 | Primary search target |
| **Micro** | ~128 токенов (~500 символов) | Точное попадание, фрагмент | Вектор | Secondary search |

### Связи:
```
Macro (chunk_index: M)
 ├── Medium (parent_chunk_index: M, chunk_index: M_C1)
 │    ├── Micro (parent_chunk_index: M_C1, chunk_index: M_C1_m1)
 │    ├── Micro (parent_chunk_index: M_C1, chunk_index: M_C1_m2)
 │    └── ...
 ├── Medium (parent_chunk_index: M, chunk_index: M_C2)
 └── ...
```

### Стратегия поиска:
1. Ищем по **Micro** для максимальной точности (exact match)
2. Ищем по **Medium** как primary target (баланс точность/контекст)
3. Возвращаем **Macro** как контекст для LLM
4. При необходимости: medium→macro resolution, micro→medium→macro resolution

---

### 1.1 Semantic Edge-Aware Chunking (рубка по естественным границам)

**Ключевое требование:** чанк не должен резать текст посередине предложения, абзаца или заголовка.
Границы чанков обязаны совпадать с естественными границами текста.

**Pipeline из RAG3 (`text_splitter.go:274-311` — `segmentsToSpans()`):**

```
Исходный текст
    ↓
Split by \n\n (параграфы)
    ↓
Для каждого параграфа:
  ├── Если isImplicitHeader() → TextSpan{IsHeader: true}
  └── Иначе → segmentSentences() → несколько TextSpan{IsNewParagraph: true для первого}
    ↓
Сборка чанков из TextSpan:
  - Заголовок → force split (начало нового чанка)
  - Превышение targetSize + есть буфер → emit + overlap
```

**Типы границ (рёбра), по которым режется текст:**

| Граница | Детектор | Поведение |
|---------|----------|-----------|
| **Markdown заголовки** `# ## ###` | `strings.HasPrefix(text, "#")` | Header → всегда split |
| **Структурные заголовки** Chapter/Part/Book/Глава/Часть/Раздел | `isImplicitHeader()` | Header → всегда split |
| **ALL-CAPS строки** | `unicode.IsUpper` для всех букв | implicit header → split |
| **Параграфы** `\n\n` | `strings.Split(text, "\n\n")` | `IsNewParagraph=true` → `\n\n` между spans |
| **Предложения** `?!.` + заглавная следующая | `([?!.]\s+)|(\n+)` + `unicode.IsUpper(next)` | Split на уровне sentence |
| **Таблицы** `|...|` | `segmentSentences()` — preserve as-is, не резать | 
| **Списки** `- `, `* `, `1. ` | `isImplicitHeader()` → return false (не резать) | Проходят как обычный текст |

**Иерархия заголовков (`text_splitter.go:204-227`):**
```go
func getHeaderLevel(text string) int {
    // Markdown: # → 1, ## → 2, ### → 3
    if strings.HasPrefix(text, "#") {
        return count('#')
    }
    // Chapter/Part/Book → 1
    if strings.HasPrefix(upper, "CHAPTER") { return 1 }
    // Everything else → 2
    return 2
}
```

**Обработка implicit headers (`text_splitter.go:229-271`):**
- `isImplicitHeader()` проверяет: NOT bold, NOT list, NOT bullet
- Chapter/Глава/Part/Часть/Book/Книга/Раздел → true
- ALL-CAPS, hasLetters, length < 100 → true

---

### 1.2 Token-Aware Overlap (перекрытие между чанками)

**Проблема:** если контекст разрывается между чанками, LLM теряет связность.

**Решение из RAG3 (`text_splitter.go:471-495`):** overlap на уровне TextSpan, не символов.

**Алгоритм:**
```
1. Текущий буфер превысил targetSize → emitChunk(currentBuffer)
2. Ищем overlap с конца буфера:
   overlapTokens = 0
   overlapStartIndex = len(buffer)
   for j = len(buffer)-1; j >= 0; j--:
       if overlapTokens + buffer[j].Tokens > overlap: break
       overlapTokens += buffer[j].Tokens
       overlapStartIndex = j
3. Новый буфер = buffer[overlapStartIndex:]
   (последние перекрывающиеся TextSpan'ы становятся началом следующего чанка)
```

**Параметры по уровням:**

| Уровень | targetSize (токенов) | Overlap (токенов) |
|---------|---------------------|--------------------|
| Macro | 2048 | 400 |
| Medium | 512 | 128 |
| Micro | 128 | 32 |

**Важно:** overlap работает на уровне TextSpan — не режется предложение или строка таблицы.
Если последний Span не влезает в overlap целиком, он не добавляется.

---

### 1.3 Semantic Zoning (позиционные ключевые слова)

### Откуда: `rag3/rag-service/service/components/semantic_zoning_test.go:13-38`

**Что делает:** добавляет в текст чанка ключевые слова о его позиции в документе,
чтобы embedding model понимала — это начало, середина или конец документа.

**Логика:**
```
posRatio = chunkIndex / (totalChunks - 1)  // 0.0 — начало, 1.0 — конец

posRatio 0.00-0.15 → ["Beginning", "Start", "Introduction", "Prologue", "Opening", "Setup"]
posRatio 0.15-0.40 → ["Early Context", "Development"]
posRatio 0.40-0.60 → ["Middle", "Midpoint", "Body"]
posRatio 0.60-0.85 → ["Climax", "Conflict", "Resolution Phase"]
posRatio 0.85-1.00 → ["Ending", "Conclusion", "Finale", "Outro", "Epilogue", "Result", "Outcome"]

Если index == 0       → добавить "First Page"
Если index == total-1 → добавить "Last Page"
```

**Формат инъекции в текст перед эмбеддингом:**
```
File: filename.pdf. Zone: Beginning, First Page. Context: Chapter 1 > Introduction. <текст чанка>
```

---

### 1.4 Структура данных TextSpan (ключевой элемент)

**Из `text_splitter.go:21-27`:**
```go
type TextSpan struct {
    Text           string   // Фрагмент текста (предложение, заголовок, строка таблицы)
    IsHeader       bool     // Это заголовок — force split на границе
    IsNewParagraph bool     // Первый span в параграфе — вставлять \n\n
    Tokens         int      // Токенов в этом span (tiktoken)
    PageNumber     int      // Номер страницы (из парсера PDF)
}
```

**Почему TextSpan важен:** это минимальная неделимая единица. Чанк НЕ может разрезать
TextSpan пополам. Границы чанка всегда совпадают с границами TextSpan.

---

### 1.5 Parent Resolution (разрешение родителей)

### Откуда: `rag3/rag-service/service/components/search_service.go:789-862`

**Алгоритм:**
1. В результатах поиска приходит `parent_chunk_index` (указывает на родителя)
2. Собираем все уникальные `(docID, parentIdx)` пары
3. Batch-загрузка из MongoDB: `GetChunksByIndices(docID, indices)`
4. Если родитель найден → возвращаем его контент + `is_parent: true`
5. Если не найден → fallback на child content + `_fallback: true`

**Для трёхуровневой системы:**
- Два прохода: micro → medium, medium → macro
- Или сразу micro → macro (через parent chain)

---

### 1.6 Что переносить из RAG3:

**Token-aware segmentation (`text_splitter.go:53-116`)**
- `segmentSentences()` — делит текст на предложения, сохраняя таблицы
- `countTokens()` — через tiktoken (cl100k_base)
- `TextSpan` — сегмент с IsHeader/IsNewParagraph/Tokens/PageNumber

**Header stack + boundary detection (`text_splitter.go:204-505`)**
- `getHeaderLevel()`, `isImplicitHeader()` — все детекторы границ
- `headerStack []HeaderNode` — иерархия заголовков
- `getCurrentHeaderChain()` → `["Chapter 1", "Section 2.1"]`
- `getCurrentChapterID()` → `"1"`, `"2"` (через regex Chapter/Глава)
- Overlap: токен-осознанный
- `emitChunk()` — формирует чанк с header_chain + chapter_id

**Three-level splitting (экстраполяция `SplitParentIntoChildren`):**
- `SplitParentIntoChildren(parent, childSize, overlap)` — рекурсивно
- Medium → Micro, Macro → Medium

**Parent resolution (`search_service.go:789-862`)**
- Batch-загрузка родителей по `parent_chunk_index`
- Fallback на child content
- Два прохода для трёхуровневой системы

**Semantic zoning (`semantic_zoning_test.go:13-38`)**
- Позиционные ключевые слова по `posRatio`
- Инъекция в текст перед эмбеддингом

### Ключевые структуры данных для переноса:
```rust
struct TextSpan {
    text: String,
    is_header: bool,
    is_new_paragraph: bool,
    tokens: usize,
    page_number: u32,
}

struct Chunk {
    text: String,
    chapter_id: String,
    header_chain: Vec<String>,
    page_number: u32,
    chunk_index: i32,
    zone_keywords: Vec<String>,     // из semantic zoning
    macro_index: Option<i32>,       // индекс macro-чанка
    medium_index: Option<i32>,      // индекс medium-чанка
    level: ChunkLevel,
}

enum ChunkLevel { Macro, Medium, Micro }
```

---

## 2. Entity-Augmented Search (Graph RAG Light)

### Откуда: `rag3/rag-service/service/components/entity_extractor.go`

**Что делает:**
- Regex `\p{Lu}\p{Ll}+(?:\s+\p{Lu}\p{Ll}+){0,2}` находит именованные сущности
- `isSentenceStart()` фильтрует false positives (заглавные в начале предложения)
- `MergeEntities()` — стемминг через Snowball, слияние под-сущностей с единственным родителем
- `AnalyzeGlobalEntities()` — dual strategy: named entities + frequent nouns
- Сущности сохраняются в поле `entities []string` каждого чанка

### Откуда: `rag3/rag-service/infrastructure/weaviate/repository.go:596-700`

**Что делает:**
- При поиске: если в запросе >1 сущности → параллельный `AND`-фильтр по полю `entities`
- Результаты тегаются `_graph_hit: true`
- Получают буст score `0.99` для видимости в топе
- Graph search НЕ заменяет, а ДОПОЛНЯЕТ векторный поиск

### Цель переноса:
- **Проблема:** векторный поиск может пропустить точное совпадение по имени/термину
- **Решение:** сущности из запроса → точный фильтр → гарантированное попадание

### Что именно переносить:

**2.1 Entity regex (`entity_extractor.go:17`)**
```rust
// \p{Lu}\p{Ll}+(?:\s+\p{Lu}\p{Ll}+){0,2}
// 1-3 капитализированных слова подряд
```

**2.2 Sentence start filter (`entity_extractor.go:20-37`)**
- `isSentenceStart()` — сканирует назад от позиции совпадения

**2.3 Stopword trim (`entity_extractor.go:86-106`)**
- Если первое слово — стоп-слово (RU/EN), отрезать его

**2.4 Stem-based merge (`entity_extractor.go:39-69`)**
- `stemParts()` — Snowball каждого слова сущности
- `isSubset()` — проверка вложенности через префиксное совпадение стемов
- `MergeEntities()` — однозначное vs многозначное слияние (защита от "Доктор → Доктор Иванов" + "Доктор Петров")

**2.5 Dual strategy extraction (`entity_extractor.go:243-327`)**
- `AnalyzeGlobalEntities()` — Named Entities (regex) + Frequent Nouns (stemming + frequency)
- Frequent nouns без пары в named получают `count = 5` (арбитраж)

**2.6 Query-time entity extraction (`entity_extractor.go:349-356`)**
- `ExtractQueryEntities()` — быстрый regex по строке запроса

**2.7 Entity co-occurrence search (`repository.go:596-700`)**
- AND-filter над полем `entities` для multi-entity запросов
- Security filter + entity filter
- Score boosting (`_graph_hit: true` → score 0.99)

---

## 3. Semantic Window Expansion

### Откуда: `rag3/rag-service/infrastructure/weaviate/repository.go:708-855`

**Что делает:**
- Для каждого найденного чанка в `V2_Document`/`V2_BotKnowledge`
- Проверяет: есть ли chunkIndex-1 и chunkIndex+1 среди результатов?
- Если нет → batch fetch через `(DocID=X AND Index=Y) OR (DocID=X AND Index=Z)`
- Сортирует всё по `DocID → ChunkIndex`

### Цель переноса:
- LLM получает не изолированные чанки, а связный контекст
- Не нужно увеличивать top-K, чтобы покрыть соседние куски
- Решает проблему разорванного контекста

### Что переносить:
- `expandWindow()` — логика dedup + сбор missing neighbors + batch fetch
- `chunkRef{DocID, Index}` + `itemMap["DocID_Index"]` для dedup
- Оптимизация: limit 20 extra chunks, fallback при слишком большом наборе

### Расширение для трёхуровневой системы:
- Semantic window работает на **Medium** уровне (основная единица поиска)
- Для Macro и Micro — аналогично, по их `chunk_index`

---

## 4. Bucket Sort (анти-"Sandwich Problem")

### Откуда: `rag3/rag-service/service/components/search_service.go:1241-1302`

**Что делает:**
- `applyBucketSort()` — группирует чанки по DocumentID сохраняя порядок релевантности

### Принцип:
1. Проходим результаты, запоминаем порядок первого появления каждого `documentID` → `rank`
2. Сортируем сначала по rank (relevance order), потом по chunk_index (reading order)
3. Чанки одного документа идут подряд, а не вперемешку с другими

### Цель переноса:
- Устраняет "sandwich problem": `<chunk from doc A> <chunk from doc B> <chunk from doc A>`
- LLM видит документы целиковыми блоками, а не кашей

### Что переносить:
- `docRank map[string]int` + `nextRank` counter
- `sort.SliceStable` с приоритетом: document rank → chunk index
- Обработка fallback: unranked документы в конец

---

## 5. Summary Chunk Injection

### Откуда: `rag3/rag-service/infrastructure/weaviate/repository.go:858-954`

**Что делает:**
- Проверяет: среди результатов есть chunk index 0 для данного документа?
- Если нет → batch fetch чанков с `index=0` для этого документа
- Вставляет с `_is_summary_injected: true`
- Лимит: 10 документов за раз (защита от большого запроса)

### Цель переноса:
- LLM получает краткое содержание документа вместе с деталями
- Контекст "о чём документ" не теряется

### Что переносить:
- `ensureSummaryChunks()` — проверка `existingSummaries` → `missingDocIDs` → batch fetch
- Ограничение: `len(operands) > 10 { operands = operands[:10] }`

---

## 6. Response Builder (Форматирование результатов)

### Откуда: `rag3/rag-service/service/components/response_builder.go`

**Что делает:**
- `GroupChunks()` — объединяет чанки одного документа в группы
- `canMerge()` — проверка: одинаковый documentID
- `formatGroupContainerized()` — оборачивает в `>>> НАЧАЛО ДОКУМЕНТА [DOC_N] <<<`

### Структура контейнера:
```
>>> НАЧАЛО ДОКУМЕНТА [DOC_1] <<<
Title: filename.pdf
Context: Chapter 1 > Section 2

>>> Context: Chapter 2 <<<
...content...

>>> Context: Chapter 3 <<<
... [Proximity Gap] ...
...content...

>>> КОНЕЦ ДОКУМЕНТА [DOC_1] <<<
```

### Дополнительно:
- Index Item (оглавление) — список всех документов с нумерацией
- `removeOverlap()` — удаление перекрытий между соседними чанками
- Gap detection: если `currentIndex != lastIndex + 1` → вставить `... [Proximity Gap] ...`
- Context change detection: если изменился header_chain → вставить `>>> Context: ... <<<`

### Цель переноса:
- Чистый, структурированный контекст для LLM
- Прозрачная навигация по источникам
- Устранение дубликатов и перекрытий

---

## 7. Post-Processing (Чистка результатов)

### Откуда: `rag3/rag-service/service/post_processing.go`

**Что делает:**
- `PostProcessResults()` — сортирует, группирует, дедуплицирует
- Subset removal: если чанк A содержит чанк B целиком → удалить B
- `trimOverlap()`: суффикс curr совпадает с префиксом next → удалить префикс next
- Формат: `<rag_search_results> ... </rag_search_results>`
- Source tracking: `[Source 1: filename.pdf | Path: Chapter 1 | Page: 5]`

### Что переносить:
- `ProcessedItem` структура
- Group by docID → sort by maxScore → внутри sort by chunkIndex
- Subset dedup (O(n²), но на <=50 чанках — ок)
- `trimOverlap()` (от max overlap до 10 символов)

---

## 8. Text Normalization Pipeline (полная)

### Откуда: `rag3/rag-service/service/components/text_processor/processor.go`

**Что делает:**

**8.1 `CleanText()` — базовая очистка (4 шага):**
1. Join hyphenated words: `(\p{L}+)-\s*\n\s*(\p{L}+)` → `$1$2` (склеивает "Ali-\nce" → "Alice")
2. Newline → space: все `\n` заменяются на пробел
3. Tab → space: все `\t` заменяются на пробел
4. Collapse whitespace: `\s+` → один пробел, затем trim

**8.2 `Tokenize()` — Unicode-токенизация:**
- `[\p{L}\d]+` — извлекает слова из любых языков (Latin, Cyrillic, и т.д.)
- toLower перед извлечением

**8.3 `DetectLanguage()` — определение языка:**
- `[\p{Cyrillic}]` — если есть хоть один кириллический символ → `"russian"`
- Иначе → `"english"`

**8.4 `AnalyzeFrequency()` — частотный анализ:**
- `CleanText()` → `DetectLanguage()` → `Tokenize()` → фильтры:
  - **Length filter:** слова < 3 рун → skip
  - **Stopword filter:** RU/EN стоп-слова → skip
  - **Numeric filter:** только цифры → skip
- **Stemming:** Snowball (RU/EN) для группировки словоформ
- **Cleanup:** стемы с частотой < 2 → отсев
- **Resolution:** стем → лучшая оригинальная форма (max frequency, tie-break: shorter)
- **Title-case:** первая буква заглавная для единообразия

**8.5 Chat-specific cleaning (`text_splitter_chat.go`):**
- Неразрывные пробелы → обычные
- Wrapped channel names ("B менеджменте..." → "менеджменте...")
- Multi-line media warning removal
- Inline time artifacts ("09:09 B" → "09:09")
- Single letter lines (PDF artifacts)
- Reaction lines (emoji/numbers)
- Forwarded time artifacts

**8.6 Structure reconstruction cleaning (`structure_reconstruction.go`):**
- Collapse multiple newlines: `\n{3,}` → `\n\n`
- Run-on header split: "Chapter One. The Beginning" → два заголовка
- ALL-CAPS lines → implicit headers
- Isolated capitalized non-sentence lines → headers

### Цель переноса:
- Единая точка нормализации перед индексацией и поиском
- Улучшение совпадения для BM25/keyword поиска
- Языковая адаптация (RU/EN)

### Что переносить:

| Компонент | Источник | Назначение |
|-----------|----------|------------|
| Hyphen join | `processor.go:35-36` | Склейка разорванных слов |
| Unicode tokenize | `processor.go:17` | `[\p{L}\d]+` |
| Language detection | `processor.go:23-29` | Cyrillic regex |
| Collapse whitespace | `processor.go:18` | `\s+` → `" "` |
| Snowball stemming | `processor.go:101-104` | Морфология RU/EN |
| Stopword maps | `stopwords/stopwords.go` | ~68 EN + ~130 RU |
| Frequency analysis | `processor.go:54-143` | Top-N концепций |
| Chat cleaning | `text_splitter_chat.go` | Специфичные паттерны |
| Table preservation | `text_splitter.go:60-79` | Не ломать markdown таблицы |

---

## 9. Stopword Fallback для Hybrid Search

### Откуда: `rag3/rag-service/infrastructure/weaviate/repository.go:514-555`

**Что делает:**
- Если Weaviate возвращает ошибку `"invalid search term, only stopwords provided"`
- Перезапускает запрос как `NearVector` (pure vector, без BM25)

### Цель переноса:
- Текущий tensor-rag: ошибка BM25 при стоп-словах → потеря всех результатов
- После: fallback на pure vector search → результаты будут

### Что переносить:
- Проверить можно ли определить ошибку "only stopword" в Qdrant
- Если да → retry без условий для BM25
- Если Qdrant не возвращает такую ошибку → fallback не нужен

---

## 10. Adaptive Context Budgeting

### Откуда: `rag3/rag-service/service/components/search_service.go:864-870`
### Детали: `rag3/rag-service/service/components/token_budget.go`

**Что делает:**
- `ApplyTokenBudget(docResults, maxTokens, botKnowledgeTokens)`
- Подсчёт токенов через tiktoken
- Если превышает `MaxContextTokens` (40k) — урезает хвост
- BotKnowledge имеет отдельный бюджет (5k)

### Цель переноса:
- Защита от переполнения контекста LLM
- Равномерное распределение токенов между документами

---

## 11. Query Pruning (Отделение истории)

### Откуда: `rag3/rag-service/service/components/search_service.go:1183-1237`

**Что делает:**
- Парсит блок `> **User History:**` в запросе
- Извлекает `realQuery` (последний запрос пользователя)
- Извлекает `relevantContext` (последние 1-2 сообщения для разрешения местоимений)

### Цель переноса:
- RAG не ищет по всему чату, а только по актуальному запросу
- Контекст для разрешения "он/она/это/выше"

---

## 12. Custom RSF (Reciprocal Score Fusion / Rerank Score Fusion)

### Откуда: нет прямой реализации в RAG3. RAG3 использует Weaviate native hybrid. Реализовать с нуля.

**Что такое RSF:** Обобщение RRF, где вместо рангов используются **нормализованные scores** из разных retrieval-каналов.

### Формула:
```
RSF(d) = Σ_{c∈channels} w_c * normalize(score_c(d))

где normalize(x) = (x - min_c) / (max_c - min_c)  — min-max нормализация в канале
или softmax(top_k) для более плавного распределения
```

### Каналы для фьюжн (tensor-rag):

| Канал | Источник | Вес (w) | Описание |
|-------|----------|---------|----------|
| Dense | Qdrant `search_points` | 0.5 | Векторная близость (cosine) |
| Sparse | Qdrant `search_points` с Qdrant sparse | 0.15 | Sparse вектор (SPLADE-like) |
| BM25 | PostgreSQL `ts_rank_cd` | 0.15 | Keyword match |
| Entity | Entity co-occurrence filter | 0.10 | Graph RAG hits |
| Structural | Header chain match / positioning | 0.05 | Секция, chapter_id совпадение |
| Recency | `uploadDate` / `timestamp` | 0.05 | Более свежие документы выше |

### Алгоритм:
1. Каждый канал возвращает `(doc_id, score, rank)`
2. Min-max нормализация scores внутри каждого канала
3. Weighted sum: `final_score(d) = Σ w_c * norm_score_c(d)`
4. Если документ отсутствует в канале → score = 0 для этого канала
5. Top-K по финальному score

### Преимущества перед RRF:
- Scores информативнее рангов: разница между top-1 и top-2 видна
- Веса каналов конфигурируемы
- Entity boost не требует отдельной логики — просто ещё один канал

### Что реализовать:
- `rsf_channel` enum: Dense, Bm25, Entity, Structural, Recency
- `RsfConfig` структура с весами и параметрами нормализации
- `compute_rsf()` — sandbox для фьюжн

---

## 13. Кластеризация результатов (UMAP → HDBSCAN)

### Откуда: нет в RAG3 (там DBSCAN). Реализовать с нуля для tensor-rag.

**Что сейчас:** DBSCAN с эвристическими параметрами (`eps=0.2`, `minPts=2`) в RAG3.

**Что требуется:** UMAP → HDBSCAN пайплайн для семантической кластеризации результатов поиска.

### Зачем:
- После поиска: 20-100 чанков от разных документов
- Кластеризация выделяет тематические группы (например: "цены", "технические детали", "сроки")
- Из каждого кластера можно взять 1-2 репрезентативных чанка → разнообразие без потери релевантности
- Позволяет реализовать **Maximal Marginal Relevance (MMR)** через кластеры: выбираем top из каждого кластера

### Pipeline:
```
Query emb + Search results → Dense vectors
    ↓
UMAP (dim reduction: 768/1536 → 5-50 dim)
    ↓
HDBSCAN (density-based, не требует eps, определяет кластеры автоматически)
    ↓
Outcome:
  - cluster_id для каждого чанка (-1 = noise)
  - centroid каждого кластера
  - расстояние от centroids до query
```

### Интеграция с RSF:
```
RSF(d) = w_dense * norm(dense_score) + w_bm25 * norm(bm25_score) + ...
       + w_cluster * cluster_diversity_bonus(d)

где cluster_diversity_bonus(d) = 1.0 если это первый выбранный из кластера
                                 0.5 если второй
                                 0.2 если третий+
```

### Что реализовать:
- Модуль `clustering.rs` с трейтом `DimensionReducer + Clusterer`
- `UmapReducer` — обёртка над `umap-rs` или самописная (PCA fallback)
- `HdbscanClusterer` — обёртка над `hdbscan-rs` или реализация
- `ClusterDiversityScorer` — интеграция с RSF для diversity

### Параметры:

| Параметр | Значение по умолч. | Описание |
|----------|--------------------|----------|
| `umap_n_neighbors` | 15 | Локальность UMAP |
| `umap_min_dist` | 0.1 | Минимальное расстояние в low-dim |
| `umap_n_components` | 10 | Целевая размерность |
| `umap_metric` | "cosine" | Метрика для UMAP |
| `hdbscan_min_cluster_size` | 2 | Мин. размер кластера |
| `hdbscan_min_samples` | 1 | Чувствительность |
| `hdbscan_cluster_selection_epsilon` | 0.0 | Слияние кластеров |
| `cluster_diversity_weight` | 0.1 | Вес diversity в RSF |

---

## 14. Починить MMR / RRF (существующие, но заглушки)

### Текущее состояние (tensor-rag):
- `vm.rs:593-621`: MMR — `scores * (1 - diversity)`, не настоящий MMR
- `vm.rs:622-636`: RRF — всегда `[0,1,2]` с фейковыми scores

### Правильный MMR:
```
MMR(d_i) = λ * Sim(q, d_i) - (1-λ) * max_{j∈S} Sim(d_i, d_j)
где S — уже выбранные документы

Алгоритм:
1. Инициализировать S = {argmax Sim(q, d_i)}
2. Пока |S| < k:
   d* = argmax_{d_i ∉ S} [λ * Sim(q,d_i) - (1-λ) * max_{j∈S} Sim(d_i,d_j)]
   S = S ∪ {d*}
```

### Правильный RRF:
```
RRF(d) = Σ_{list ∈ ranked_lists} 1 / (k + rank(list, d))

где k = 60 (константа сглаживания)
ranked_lists — упорядоченные списки из разных retrieval-каналов
```

### Примечание:
- Если RSF (раздел 12) реализован, RRF и MMR становятся опциональными
- RSF естественно покрывает оба случая через weighted sum + cluster diversity

---

## Приоритетность

| Приоритет | Компонент | Влияние | Сложность |
|-----------|-----------|---------|-----------|
| **P0** | Трёхуровневое чанкование (Macro/Medium/Micro) | Точность + контекст | Средняя |
| **P0** | Entity-Augmented Search | Точные совпадения | Низкая |
| **P0** | Custom RSF (вместо сломанных RRF/MMR) | Ранжирование | Средняя |
| **P1** | Bucket Sort | Качество контекста LLM | Низкая |
| **P1** | Semantic Window | Связность контекста | Средняя |
| **P1** | Summary Injection | Контекст документа | Низкая |
| **P2** | Text Normalization Pipeline | Качество индексации | Средняя |
| **P2** | Response Builder | Чистота контекста | Средняя |
| **P2** | Post-Processing | Дедупликация | Низкая |
| **P2** | Кластеризация (UMAP → HDBSCAN) | Diversity | Высокая |
| **P3** | Stopword Fallback | Надёжность поиска | Низкая |
| **P3** | Context Budgeting | Защита LLM | Средняя |
| **P3** | Query Pruning | Чистота запроса | Низкая |

---

## Файлы-источники (полные пути)

| Компонент | Файл в RAG3 |
|-----------|------------|
| Parent/Child chunking | `Private--test--RAG3/rag-service/service/components/text_splitter.go` |
| Entity extraction | `Private--test--RAG3/rag-service/service/components/entity_extractor.go` |
| Entity search in Weaviate | `Private--test--RAG3/rag-service/infrastructure/weaviate/repository.go` |
| Search service (bucketing, parent resolution) | `Private--test--RAG3/rag-service/service/components/search_service.go` |
| Post-processing | `Private--test--RAG3/rag-service/service/post_processing.go` |
| Response builder | `Private--test--RAG3/rag-service/service/components/response_builder.go` |
| Text processor | `Private--test--RAG3/rag-service/service/components/text_processor/processor.go` |
| Text processor tests | `Private--test--RAG3/rag-service/service/components/text_processor/processor_test.go` |
| Stopwords (RU + EN) | `Private--test--RAG3/rag-service/service/components/stopwords/stopwords.go` |
| Token budgeting | `Private--test--RAG3/rag-service/service/components/token_budget.go` |
| Query pruning | `Private--test--RAG3/rag-service/service/components/search_service.go:1183-1237` |
| Structure extraction | `Private--test--RAG3/rag-service/service/components/structure_extractor.go` |
| Structure reconstruction | `Private--test--RAG3/rag-service/service/components/structure_reconstruction.go` |
| Chat text splitter | `Private--test--RAG3/rag-service/service/components/text_splitter_chat.go` |
| Semantic zoning | `Private--test--RAG3/rag-service/service/components/semantic_zoning.go` |
| Domain models | `Private--test--RAG3/rag-service/domain/models.go` |
| Config (all defaults) | `Private--test--RAG3/rag-service/config/config.go` |
| DBSCAN native | `Private--test--RAG3/rag-service/infrastructure/ml/native/engine.go:249-490` |

---

## Целевые файлы в tensor-rag

| Компонент | Целевой файл |
|-----------|-------------|
| Трёхуровневое чанкование | `tensor-rag/src/chunker.rs` |
| Search strategies + RSF | `tensor-rag/src/ir.rs` + `tensor-rag/src/vm.rs` |
| Entity extraction + search | Новый: `tensor-rag/src/entity.rs` |
| Bucket sort + post-processing | Новый: `tensor-rag/src/response.rs` |
| Text normalization | Новый: `tensor-rag/src/normalizer.rs` |
| Кластеризация (UMAP → HDBSCAN) | Новый: `tensor-rag/src/clustering.rs` |
| Search orchestration | `tensor-rag/src/mcp.rs` |
| Stopwords | Новый: `tensor-rag/src/stopwords.rs` |
