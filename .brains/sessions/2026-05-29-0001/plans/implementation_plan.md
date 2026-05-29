# План реализации: "Тензорная машина"

## Фаза 1: MVP-ядро (4 недели)

### Неделя 1 — MCP-сервер и транспортный слой

**Задача 1.1. MCP-сервер на Rust**

- Крейт `mcp-server` с зависимостями: `tokio`, `tonic` (gRPC), `serde`/`serde_json`.
- Реализация **Model Context Protocol** (спецификация Anthropic): `McpServer` поддерживает долгоживущие `stream`-соединения.
- Типы: `McpRequest { tool: String, args: JsonValue, session_id: String }`, `McpResponse { status: u32, body: Vec<u8> }`.
- Регистрация инструментов через `trait ToolHandler`: `fn handle(&self, args: JsonValue) -> Result<McpResponse>`.
- Первый инструмент: `search_simple(query, top_k, filters)` — входная точка для LLM-агентов.

**Задача 1.2. Асинхронный пул соединений**

- Крейт `storage`.
- `QdrantPool` — пул gRPC-коннекторов к Qdrant (через `tonic`). Настраиваемый размер пула, health-check, retry с экспоненциальной задержкой.
- `PostgresPool` — обёртка над `sqlx::PgPool`.
- Трейт `VectorStore`: `fn search(&self, query: &[f32], top_k: usize, filters: &[Filter]) -> Result<Vec<SearchResult>>`.
- Трейт `MetadataStore`: `fn fetch_scores(&self, ids: &[u64]) -> Result<Vec<Bm25Score>>`.

**Ключевое решение:** на этом этапе копирование векторов допустимо. gRPC → protobuf → Vec<f32>.

### Неделя 2 — IR (Intermediate Representation)

**Задача 2.1. Определение IR**

- Крейт `ir`.
- `enum Instruction`: `TopK { k: u32 }`, `Add`, `Mul`, `MulScalar(f32)`, `Fusion { weights: Vec<f32> }`, `LoadConst(f32)`, `Emit`.
- `struct Program { instructions: Vec<Instruction> }` — линейная последовательность (без циклов на этом этапе).
- `struct Tensor { data: Vec<f32>, shape: Vec<usize> }` — временное представление.

**Задача 2.2. JSON-парсер запросов → IR**

- Крейт `dsl` (временная заглушка для JSON).
- `JsonQueryParser`: десериализует `{ "query": "...", "top_k": 5, "weights": {"sem": 0.7, "bm25": 0.3} }`.
- Генератор IR: `fn compile(query: JsonQuery) -> Program`.
- Правила трансляции:
  - `semantic_scores = dot_product(vectors, query_vector)` → 3 инструкции (load_const для -1? Нет, dot product — это mul + add по осям. Пока оставляем как `Fusion`).
  - `final_scores = sem*ws + bm25*wb` → `MulScalar sem, ws`, `MulScalar bm25, wb`, `Add`, `TopK`, `Emit`.

**Задача 2.3. VM-исполнитель (5 инструкций)**

- Крейт `vm`.
- `struct Vm { program: Program }`.
- `fn execute(&self, inputs: HashMap<String, Tensor>) -> Result<HashMap<String, Tensor>>`.
- Реализация 5 инструкций:
  - `MulScalar(tensor, scalar)` — поэлементное умножение `tensor.iter().map(|x| x * scalar)`.
  - `Add(a, b)` — поэлементное сложение.
  - `TopK(tensor, k)` — частичная сортировка через `select_nth_unstable` (O(n)), сбор top-k.
  - `Fusion(sem, bm25, w_sem, w_bm25)` — `sem * w_sem + bm25 * w_bm25`.
  - `Emit(tensor)` — упаковка результата в FlatBuffer.
- **Тесты:** unit-тесты на каждый инструкцию. Бенчмарк: 1000 элементов → top-5 < 1 µs.

### Неделя 3 — Интеграция Qdrant + PostgreSQL

**Задача 3.1. Прямой gRPC-вызов к Qdrant**

- Используем `qdrant-client` (Rust SDK от Qdrant) или пишем protobuf-клиент вручную.
- `QdrantService::ann_search(collection, query_vector, top_k, filter)` → возвращает `Vec<(u64, f32)>` (ID, score).
- Фильтры передаются как Qdrant `Condition` (predicate pushdown: `year >= 2023` → `FieldCondition("year", Range(gte: 2023))`).

**Задача 3.2. Загрузка BM25-скоров из PostgreSQL**

- Таблица `documents`: `id BIGINT PRIMARY KEY`, `text TEXT`, `bm25_score REAL`, `metadata JSONB`.
- `PostgresService::fetch_scores(ids: &[u64])` → `Vec<f32>` (в том же порядке, что и ids).
- Загрузка — одним `SELECT ... WHERE id = ANY($1) ORDER BY array_position($1, id)` для сохранения порядка.

**Задача 3.3. Склейка данных и вызов VM**

- `SearchPipeline::run(query, filters, top_k)`:
  1. Параллельно: `qdrant_search()` и (опционально) `postgres_fetch_metadata()`.
  2. Формирование `HashMap<"sem_scores" | "bm25_scores", Tensor>`.
  3. Вызов `Vm::execute(program, inputs)`.
  4. Возврат списка `(id, score)`.
- Интеграционный тест: запуск локального Qdrant и PostgreSQL через `testcontainers`.

### Неделя 4 — Сквозной пайплайн и бенчмарки

**Задача 4.1. Замыкание MCP → SearchPipeline → LLM**

- Инструмент `search_simple` в MCP-сервере вызывает `SearchPipeline::run`.
- Результат форматируется как MCP-ответ: `McpResponse { body: top5_texts }`.

**Задача 4.2. Критериальные бенчмарки**

- `cargo bench` с `criterion`:
  - Latency P50/P95/P99 для 100, 500, 1000 кандидатов.
  - Memory usage (через `alloc` counter или `jemalloc` stats).
- Цель: P99 < 5 мс на 1000 кандидатов (без сети, только VM).
- Профилировка через `perf` / `flamegraph` для выявления горячих точек.

**Задача 4.3. CLI-утилита для отладки**

- `tensor-machine-cli --query "..." --top-k 5`.
- Вывод IR-плана и результата.

**Milestone Фазы 1:** Рабочий эндпоинт, принимающий JSON-запрос и возвращающий Top-5 текстов через MCP. VM считает гибридный реранк за < 1 µs на 1000 элементах.

---

## Фаза 2: Добавление DSL (2 недели)

### Неделя 5 — Внедрение steel-scheme

**Задача 5.1. Интеграция `steel-scheme`**

- Добавление `steel` в зависимости.
- `SteelRuntime` — обёртка, инициализирующая VM steel со стандартными библиотеками.
- Регистрация Rust-функций в steel: `(register-rust-fn "qdrant-search" qdrant_search_rust_fn)`.
- Функция `eval_search(query_s_expr: &str) -> Program`: парсит s-выражение, вызывает steel-функцию `compile-query`.

**Задача 5.2. Макросы планировщика**

- `(search :query q :filters f :retrieval r :rerank rr :final-top-k k)` → разворачивается в последовательность примитивов.
- `(hybrid :weights (sem w1 bm25 w2))` → разворачивается в `mul + mul + add`.
- `(filter-predicate expr)` — трансляция s-выражения в Qdrant filter protobuf.
- Все макросы пишутся на **самом Scheme**, встроенном в steel.

**Задача 5.3. REPL для отладки запросов**

- Интерактивная консоль: ввод `(search ...)` → вывод IR-байткода + результата.
- Полезно для data scientist'ов, экспериментирующих со стратегиями.

### Неделя 6 — Multi-hop и сложные стратегии

**Задача 6.1. Макрос multi-hop**

```scheme
(define-macro (multi-hop query hops)
  `(let loop ((q ,query) (results '()) (remaining ,hops))
     (if (zero? remaining)
         (reverse results)
         (let ((batch (search q)))
           (loop (generate-next-query q batch)
                 (cons batch results)
                 (- remaining 1))))))
```

- Планировщик разворачивает это в N последовательных IR-программ, где каждая следующая получает данные предыдущей.

**Задача 6.2. Условная логика (Self-RAG)**

- Макрос `(if-confidence score threshold low-confidence-plan high-confidence-plan)`.
- Во время исполнения Rust проверяет условие (максимальный score > threshold) и выбирает ветвь.

**Задача 6.3. JSON-резерв**

- Для совместимости: `DualParser` — определяет формат входа (JSON или s-expression).
- Внутренне оба преобразуются в единый `AstNode`, который компилируется в IR.

**Milestone Фазы 2:** LLM-агент может отправлять s-выражения (или JSON) с любыми комбинациями фильтров, реранка и multi-hop. Новая стратегия добавляется как Scheme-макрос без перекомпиляции Rust.

---

## Фаза 3: Zero-Copy (2 недели)

### Неделя 7 — Интеграция Apache Arrow

**Задача 7.1. Arrow RecordBatch в конвейере**

- Зависимости: `arrow` (или `arrow2`), `polars` (опционально).
- `ArrowBuilder::from_qdrant_response(bytes: Bytes)` — zero-copy представление.
- Схема: `FixedSizeList<Float32>[dim]` для векторов, `Float32` для BM25, `Int64` для ID, `Utf8` для текста.
- `RecordBatch` хранится в `Arc<RecordBatch>`.

**Задача 7.2. Адаптация VM под Arrow**

- Вместо `Tensor { data: Vec<f32> }`, VM работает с `ArrayRef` (из `arrow::array`).
- `ArrowVm` — новая реализация тех же инструкций, но через `arrow::compute`:
  - `add(a: &Float32Array, b: &Float32Array)` → `arrow::compute::add(a, b)`.
  - `mul` / `mul_scalar` → `arrow::compute::multiply`.
  - `topk` — кастомная реализация над `Float32Array` (используем `indices_sorted_by`).
- **Бенчмарк:** сравнить скорость Vec<T> vs Arrow (ожидается ~равно или Arrow быстрее за счёт векторизации).

**Задача 7.3. FlatBuffer для результата**

- `flatbuffers` (или `planus`) для компактного вывода.
- Схема: `table SearchResult { ids: [int64], scores: [float32] }`.
- Размер результата для top-5: ~60 байт.

### Неделя 8 — mmap-регионы

**Задача 8.1. Shared Memory Manager**

- Крейт `shared-memory`.
- `MmapRegion<T>` — обёртка над `memmap2`.
- Создание буфера: `MmapRegion::new(size_in_bytes)` → mmap `/dev/shm` (Linux) или анонимный mmap.
- Регистрация в глобальном реестре: `HashMap<String, Arc<MmapRegion>>`.
- Передача указателя: `region.as_ptr() -> *const u8` (внутрипроцессно, без копирования).

**Задача 8.2. Протокол обмена**

- Если VM решили вынести в отдельный процесс (необязательно):
  - Данные пишутся в mmap-регион.
  - Передаётся `(region_id, offset, length)` через Unix domain socket.
  - Получатель маппит тот же регион и работает с данными напрямую.

**Задача 8.3. Интеграция с Arrow**

- Arrow IPC Format: `RecordBatch` → `arrow::ipc::writer::FileWriter` → mmap-буфер.
- `MemoryMappedArrow::open(region)` → отображает IPC-файл как Arrow-таблицу.
- Таким образом, данные из сети (Qdrant gRPC) → Arrow → mmap → VM без единой копии.

**Milestone Фазы 3:** Конвейер Qdrant → Arrow → VM работает без промежуточных копий. Измерение: latency drop на 30-50% для батчей > 1000 векторов.

---

## Фаза 4: Production-фичи (непрерывно)

### Поток 1 — GPU-бэкенд через WGPU (3-4 недели)

**Задача П1.1. WGPU-ядро**

- Крейт `vm-gpu`.
- Реализация тех же инструкций на WGSL (WebGPU Shading Language).
- `WgpuVm::execute(program, inputs)`:
  1. Загрузка тензоров в GPU-буферы (через `wgpu::Buffer`).
  2. Компиляция IR в WGSL-шадер (один dispatch на инструкцию).
  3. Чтение результата через `read_buffer` (асинхронный `map_async`).

**Задача П1.2. Автовыбор бэкенда**

- `VmFactory::new()`:
  - Пытается инициализировать `wgpu::Instance`.
  - Если GPU недоступен → падает на CPU-бэкенд (`ArrowVm`).
  - Если доступен → выбирает GPU для батчей > 5000 (точка crossover).

**Задача П1.3. Бенчмарки CPU vs GPU**

- Сравнение latency/throughput для размеров батча: 100, 1000, 10000, 100000.
- Определение порогов (crossover point).

### Поток 2 — ColBERT и late interaction (2-3 недели)

**Задача П2.1. Тензорный формат для ColBERT**

- Каждый документ представлен как `[token_count, dim]` тензор.
- Arrow-схема: `List<FixedSizeList<Float32>[dim]>`.
- Хранение: LanceDB или кастомный Arrow-файл.

**Задача П2.2. Инструкция `colbert_maxsim`**

- Вход: `query_tensor: [q_len, dim]`, `doc_tensors: List<[d_len, dim]>`.
- Алгоритм: для каждого документа:
  1. MatMul(query, doc^T) → `[q_len, d_len]` матрица схожести.
  2. Max по оси d_len → `[q_len]` (maxSim per query token).
  3. Sum по оси q_len → скаляр.
- Реализация на CPU: через `ndarray` + SIMD (или `faer`).
- Реализация на GPU: WGSL-шадер с распараллеливанием по документам.

**Задача П2.3. DSL-макрос для ColBERT**

```scheme
(search :query q :retrieval (colbert :top 100) :rerank (identity) :final-top-k 10)
```

### Поток 3 — Горизонтальное масштабирование (3-4 недели)

**Задача П3.1. Arrow Flight SQL**

- `arrow-flight` крейт для gRPC-обмена Arrow-данными между нодами.
- `FlightServer` — публикует `RecordBatch` с результатами поиска для ребалансировки.

**Задача П3.2. Координатор MCP-серверов**

- Легковесный `Coordinator` на `etcd` или `redis` (через `redis-rs`).
- Регистрация нод: сердцебиение каждые 5 сек.
- Маршрутизация: `session_id.hash() % num_nodes` — консистентное хеширование.

**Задача П3.3. Шардирование индекса**

- Qdrant уже поддерживает шардирование. Наша задача — правильная конфигурация.
- Разделение коллекций: `shard_id = doc_id % num_shards`.
- `SearchPipeline::run` посылает запрос во все шарды, объединяет top-k (merge sort на Rust).

### Поток 4 — Observability и reliability (непрерывно)

**Задача П4.1. Трассировка (OpenTelemetry)**

- Инструментирование: `tracing` + `opentelemetry-otlp`.
- Спаны: `mcp_request`, `plan_compile`, `qdrant_search`, `postgres_fetch`, `vm_execute`.
- Метрики: `vm_latency_histogram`, `qdrant_latency_histogram`, `error_counter`.

**Задача П4.2. Graceful degradation**

- Если Qdrant timeout → fallback на BM25-только ранжирование.
- Если PostgreSQL недоступен → ранжирование только по семантическим скорам.
- Реализация через `tokio::time::timeout` и `fallback` в `SearchPipeline`.

**Задача П4.3. Rate limiting и backpressure**

- `governor` (token bucket) на уровне MCP-сервера.
- Per-session rate limit.
- Backpressure через bounded channels (`tokio::sync::mpsc::channel`).

---

## График и зависимости

```
Неделя 1  ████████  MCP-сервер + пулы
Неделя 2  ████████  IR + VM (5 инструкций)
Неделя 3  ████████  Qdrant + PostgreSQL интеграция
Неделя 4  ████████  Сквозной пайплайн + бенчмарки
          ───────── ФАЗА 1: MVP готов
Неделя 5  ████████  steel-scheme + базовые макросы
Неделя 6  ████████  Multi-hop, условная логика, JSON-резерв
          ───────── ФАЗА 2: DSL готов
Неделя 7  ████████  Arrow RecordBatch
Неделя 8  ████████  mmap + zero-copy конвейер
          ───────── ФАЗА 3: Zero-Copy готов
Неделя 9+  ████████  GPU (WGPU), ColBERT, масштабирование, observability
          ───────── ФАЗА 4: Production-фичи (параллельные потоки)
```

---

## Ключевые метрики успеха

| Фаза | Метрика | Критерий приёмки |
|------|---------|------------------|
| 1 | P99 latency (1000 candidates, VM only) | < 1 µs |
| 1 | P99 latency (full pipeline, no network) | < 1 ms |
| 1 | Memory per 10k sessions | < 100 MB (цель 500 MB на все 10k) |
| 2 | Время добавления новой стратегии | < 1 час (макрос, без компиляции Rust) |
| 3 | Доля копирований в pipeline | 0 (zero-copy) |
| 3 | Ускорение relative to Phase 1 | ≥ 1.5× на батчах > 1000 |
| 4 | GPU throughput vs CPU | ≥ 3× на батчах > 10000 |
| 4 | Throughput кластера | ≥ 50k req/s |

---

## Риски и mitigation

| Риск | Вероятность | Влияние | Mitigation |
|------|------------|---------|------------|
| Qdrant gRPC latency выше ожиданий | Средняя | Высокое | Асинхронный prefetch, конвейеризация запросов |
| Arrow overhead перевешивает zero-copy выгоду | Низкая | Среднее | Бенчмарки на неделе 7, fallback на Vec<f32> |
| steel-scheme не поддерживает нужные макросы | Средняя | Среднее | Резерв — самописный парсер s-expr |
| WGPU не даёт прироста на малых батчах | Высокая | Низкое | CPU-бэкенд по умолчанию, GPU только > crossover |
| Сложность отладки IR-программ | Высокая | Среднее | REPL, трассировка, дамп IR перед исполнением |

---

Этот план покрывает ~12 недель разработки до production-grade системы с полным стеком: MCP → DSL → IR → Tensor VM (CPU/GPU) → Zero-Copy Arrow pipeline → горизонтальное масштабирование.
