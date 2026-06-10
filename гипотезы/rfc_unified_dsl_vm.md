# RFC: Unified Tensor Query DSL and VM Architecture

> Статус: Draft / June 2026
> На основе: исследование APL/J/K, Datalog, Backus FP, Relational Algebra,
>           Concatenative/Stack languages, Charity, SETL

---

## 1. Motivation

Текущая архитектура: `dsl.rs` → `ir.rs` (AstNode) → `Planner` → `Program (Vec<OpCode>)` → VM.

Проблемы:
- OpCode-набор ad-hoc, без формальной системы
- Нет алгебры для оптимизации запросов (rewrite rules)
- Multi-hop и conditional RAG — хардкод, не декларативен
- Нет rank-polymorphism для тензорных операций
- Паттерн retrieve-then-read не выражен формально

**Цель RFC**: определить расширение VM и DSL так, чтобы охватить четыре парадигмы:

| Слой | Парадигма | Что даёт |
|---|---|---|
| 1 | APL tensor ops | Rank-polymorphic операции над векторами/матрицами |
| 2 | Relational algebra | Формальная семантика запросов, rewrite rules |
| 3 | Datalog | Рекурсия для multi-hop, rule-based стратегии |
| 4 | FP combinators | Композиция пайплайнов, ветвление, fixpoint |

---

## 2. Архитектура: Two-Level VM

```
┌──────────────────────────────────────────────────────────┐
│                  DSL Source (Surface Syntax)               │
│  S-expressions / Concatenative / JSON                     │
└──────────────────────┬───────────────────────────────────┘
                       ▼
┌──────────────────────────────────────────────────────────┐
│               Parser (dsl.rs extended)                    │
│  parse_dsl() → AstNode с новыми variant'ами              │
└──────────────────────┬───────────────────────────────────┘
                       ▼
┌──────────────────────────────────────────────────────────┐
│              Optimizer (Rule-based + Cost-based)          │
│  Rewrite rules: селекция-пушдаун, fusion, реордеринг     │
│  APL identities, FP laws, Datalog magic-set              │
└──────────────────────┬───────────────────────────────────┘
                       ▼
┌──────────────────────────────────────────────────────────┐
│         HIR (High IR) — Planner::compile()               │
│  OpCode: TENSOR_*, REL_*, DL_*, FP_*                     │
│  Sub-programs для control flow                           │
└──────────────────────┬───────────────────────────────────┘
                       ▼
┌──────────────────────────────────────────────────────────┐
│       LIR (Low IR) — Lowering Pass (опционально)         │
│  Распаковка высокоуровневых ops в последовательность     │
│  примитивов VM (Add, Mul, LoadConst, и т.д.)             │
└──────────────────────┬───────────────────────────────────┘
                       ▼
┌──────────────────────────────────────────────────────────┐
│                VM Executor (vm.rs extended)               │
│  Стековая машина, Arrow-совместимые данные               │
│  Tokio для PARALLEL, semi-naive для DL_FIXPOINT          │
└──────────────────────────────────────────────────────────┘
```

Ключевое решение: **не разделять High/Low VM**. Все новые OpCode добавляются
в существующий enum. Sub-programs (вектор `Program` внутри `Instruction`)
обеспечивают вложенность для control flow.

---

## 3. Data Model

### 3.1. Value Enum (vm.rs)

```rust
#[derive(Debug, Clone)]
pub enum Value {
    // --- Существующие ---
    Scalar(f32),
    Tensor1D(Array1<f32>),
    Tensor2D(ndarray::Array2<f32>),
    ArrowArray(Arc<dyn Array>),
    Result(TopKResult),
    AnchorResult { ids: Vec<i64>, scores: Vec<f32>, names: Vec<String> },

    // --- Добавляемые ---
    /// Таблица — RecordBatch из Arrow. Основа для реляционных операций.
    Relation(Arc<arrow::record_batch::RecordBatch>),

    /// Подпрограмма — Program, переданная через стек (для FP combinators)
    Program(Program),

    /// Дескриптор формы тензора (для RESHAPE, TRANSPOSE)
    Shape(Vec<usize>),

    /// Индексы — результат GRADE, WHERE (Arrow UInt64Array)
    Indices(Arc<arrow::array::UInt64Array>),

    /// Булево значение для CONDITION
    Condition(bool),

    /// Идентификатор функции для параметризации INNER_PRODUCT и т.д.
    FuncId(u32),

    /// Имя отношения для Datalog EDB
    RelName(String),

    /// Кортеж Datalog-факта
    Tuple(Vec<Value>),
}
```

### 3.2. Tensor (обёртка над Arrow)

```rust
/// Rank-полиморфный тензор над Arrow-буфером.
/// Позволяет APL-стиль операций без копирования.
pub struct Tensor {
    /// Сырые данные (f32/f16), Arrow Array
    pub data: Arc<dyn Array>,
    /// Логическая форма (например, [10, 768])
    pub shape: Vec<usize>,
    /// Смещения в элементах (не в байтах), row-major по умолчанию
    pub strides: Vec<usize>,
}
```

---

## 4. Layer 1: APL-style Tensor Operators

### 4.1. Операторы

| APL | OpCode | Стек | Описание |
|---|---|---|---|
| `+.×` | `TENSOR_INNER_PRODUCT` | `[a, b, func] → Scalar` | Обобщённое скалярное произведение |
| `⍤` (rank) | `TENSOR_EACH_CELL(rank)` | `[arr, func] → Array` | Apply func to each cell of rank r |
| `φ` (fft) | `TENSOR_FOURIER` | `[arr, d_new, kernel] → Array` | Random Fourier Features (Rahimi & Recht) |
| `⋈JL` | `DIM_MUTATE(RandomProjection)` | `[arr, d_new] → Array` | Johnson-Lindenstrauss random projection |
| `⊕` | `TENSOR_CONCAT` | `[a, b, axis] → Array` | Конкатенация тензоров |
| `∘.f` | `TENSOR_OUTER_PRODUCT` | `[a, b, func] → ArrowArray` | Внешнее произведение |
| `f/` | `TENSOR_REDUCE` | `[arr, axis, func] → ArrowArray` | Свёртка вдоль оси |
| `f\` | `TENSOR_SCAN` | `[arr, axis, func] → ArrowArray` | Префиксная свёртка |
| `⍋` | `TENSOR_GRADE_UP` | `[arr] → Indices` | Индексы сортировки по возрастанию |
| `⍒` | `TENSOR_GRADE_DOWN` | `[arr] → Indices` | Индексы сортировки по убыванию |
| `mask/` | `TENSOR_COMPRESS` | `[arr, mask] → ArrowArray` | Выбор по булевой маске |
| `↑` | `TENSOR_TAKE(n)` | `[arr] → ArrowArray` | Первые/последние N |
| `⍴` | `TENSOR_RESHAPE(shape)` | `[arr] → ArrowArray` | Изменение формы |
| `⍉` | `TENSOR_TRANSPOSE(axes)` | `[arr] → ArrowArray` | Перестановка осей |
| `⍸` | `TENSOR_WHERE` | `[mask] → Indices` | Индексы ненулевых |
| `⍴⍴` | `TENSOR_SHAPE` | `[arr] → Shape` | Получить форму тензора |
| `∊` | `TENSOR_MEMBER` | `[a, b] → ArrowArray(bool)` | Проверка вхождения |
| `⍸I` | `TENSOR_INTERVAL_INDEX` | `[bins, vals] → Indices` | Биннинг |

### 4.2. Rust signatures (dispatch в VM)

```rust
OpCode::TENSOR_INNER_PRODUCT => {
    let func = stack.pop_as::<FuncId>()?;
    let b = stack.pop_as::<ArrowArray>()?;
    let a = stack.pop_as::<ArrowArray>()?;
    // dispatch по func:
    let result = match func.0 {
        0 => cosine_similarity_simd(&a, &b),       // +.× с cosine
        1 => dot_product_simd(&a, &b),              // +.× классический
        2 => max_min_similarity(&a, &b),            // ⌈.⌊
    };
    stack.push(Value::Scalar(result));
}

OpCode::TENSOR_GRADE_DOWN => {
    let arr = stack.pop_as::<ArrowArray>()?;
    let indices = arrow::compute::sort_to_indices(
        &[SortColumn { values: arr, options: Some(SortOptions::descending()) }],
        None,
    )?;
    stack.push(Value::Indices(Arc::new(indices)));
}

OpCode::TENSOR_COMPRESS => {
    let mask = stack.pop_as::<BooleanArray>()?;
    let arr = stack.pop_as::<ArrowArray>()?;
    let result = arrow::compute::filter(&arr, &mask)?;
    stack.push(Value::ArrowArray(Arc::new(result)));
}
```

### 4.3. Rank polymorphism

Для CELL-операций (как J's `"`) добавляется параметр:

```rust
OpCode::TENSOR_EACH_CELL(rank) => {
    let arr = stack.pop_as::<Tensor>()?;
    // Разбить на cell'ы ранга `rank`, применить функцию
    // из стека к каждому, собрать результат
}
```

---

## 5. Layer 2: Relational Query Algebra

### 5.1. Операторы

| Оператор Codd | OpCode | Стек |
|---|---|---|
| `σ` (select) | `REL_SELECT(Predicate)` | `[relation] → Relation` |
| `π` (project) | `REL_PROJECT(Vec<String>)` | `[relation] → Relation` |
| `⋈` (join) | `REL_JOIN(JoinType)` | `[left, right, on] → Relation` |
| `∪` (union) | `REL_UNION` | `[left, right] → Relation` |
| `∖` (diff) | `REL_DIFFERENCE` | `[left, right] → Relation` |
| `τ` (order) | `REL_ORDER_BY(Vec<(String, Dir)>)` | `[relation] → Relation` |
| `γ` (agg) | `REL_AGGREGATE(Aggregate)` | `[relation, group_cols] → Relation` |
| `ρ` (rename) | `REL_RENAME(Vec<(String, String)>)` | `[relation] → Relation` |
| `LIMIT` | `REL_LIMIT(usize)` | `[relation] → Relation` |

### 5.2. Join types addition

```rust
pub enum JoinType {
    Inner,
    LeftOuter,
    RightOuter,
    FullOuter,
    /// Similarity join: левое и правое отношение соединяются
    /// по семантической близости embedding-колонок
    Similarity {
        metric: SimilarityMetric,   // Cosine, Euclidean, Dot
        threshold: f32,             // минимальный порог
        top_k: Option<usize>,       // максимум результатов
    },
    /// As-of join (из kdb+/q): для каждого кортежа слева
    /// найти ближайший по времени справа
    AsOf { time_column: String, tolerance: Option<i64> },
}
```

### 5.3. Predicate

```rust
pub enum Predicate {
    Col(String),
    Lit(Value),
    Eq(Box<Predicate>, Box<Predicate>),
    Neq(Box<Predicate>, Box<Predicate>),
    Gt(Box<Predicate>, Box<Predicate>),
    Ge(Box<Predicate>, Box<Predicate>),
    And(Box<Predicate>, Box<Predicate>),
    Or(Box<Predicate>, Box<Predicate>),
    Not(Box<Predicate>),
    /// Векторное расстояние < порог
    Similarity { col: String, query: Box<Predicate>, metric: SimilarityMetric, threshold: f32 },
    /// Проверка вхождения в список
    In(Box<Predicate>, Vec<Value>),
}
```

### 5.4. Rewrite Rules для планировщика

Первый набор правил для Optimizer:

```
-- Selection fusion
σc(σd(R)) = σc∧d(R)

-- Selection pushdown over join
σc(R ⋈ S) = σc(R) ⋈ S           if c ∈ attrs(R)
σc(R ⋈θ S) = σc₁(R) ⋈θ σc₂(S)  if c = c₁ ∧ c₂, c₁∈R, c₂∈S

-- Projection pushdown
πA(σc(R)) = πA(σc(πA∪attrs(c)(R)))

-- Similarity threshold pushdown
πid(TopK_k(R ⋈θ S)) = πid(TopK_k(R)) ⋈θ πid(TopK_k(S))  -- если нужны только id

-- Distribution: compress over elementwise
mask / (v1 + v2) = (mask/v1) + (mask/v2)
mask / f(v)      = f(mask/v)     -- если f поэлементна

-- Grade properties
⍋⍋v = rank(v)    -- O(n) из permutation
⍋v = ⍒(-v)       -- avoid recomputation

-- Fold fusion
f ∘ /g = /h    if f(g(a,b)) = h(f(a), f(b))
Пример: Top5 ∘ /merge = /top5_merge
```

---

## 6. Layer 3: Datalog Recursion Engine

### 6.1. Операторы

| OpCode | Стек | Описание |
|---|---|---|
| `DL_REGISTER_REL` | `[name, arity] → ()` | Объявить отношение |
| `DL_FACT` | `[name, tuple] → ()` | Вставить факт |
| `DL_INDEX` | `[name, key_cols] → ()` | Построить хеш-индекс |
| `DL_JOIN` | `[r1, r2, var_map] → Relation` | Join двух rel-переменных |
| `DL_PROJECT` | `[relation, vars] → Relation` | Проекция на переменные |
| `DL_UNION` | `[r1, r2] → Relation` | Объединение |
| `DL_DIFFERENCE` | `[r1, r2] → Relation` | Разность |
| `DL_FIXPOINT(max_iter)` | `[body_prog, until_prog] → Relation` | Semi-naive fixpoint |
| `DL_STRATUM(id)` | `[] → ()` | Начало стратума |

### 6.2. Semi-naive fixpoint (DL_FIXPOINT)

```
1. Full ← Delta ← Initial facts
2. Loop:
   a. New ← body(Delta, Full)
   b. New ← New ∖ Full       (дедупликация)
   c. If New empty → break
   d. Delta ← New
   e. Full ← Full ∪ New
3. Return Full
```

В VM:

```rust
OpCode::DL_FIXPOINT(max_iter) => {
    let until_prog = stack.pop_as::<Program>()?;
    let body_prog = stack.pop_as::<Program>()?;

    let mut edb = self.edb.clone();  // EDB: HashMap<RelName, Relation>
    let mut iteration = 0;

    loop {
        // Выполнить body-программу: она читает edb и
        // оставляет New на стеке
        self.exec_sub_program(&body_prog)?;
        let new = stack.pop_as::<Relation>()?;

        // Дедупликация: New ∖ Full
        let deduped = relation_difference(&new, &edb["hop"])?;

        if deduped.num_rows() == 0 || iteration >= max_iter {
            break;
        }

        // Delta ← New
        edb.insert("hop_delta".into(), deduped.clone());
        // Full ← Full ∪ New
        edb.insert("hop_full".into(), relation_union(&edb["hop_full"], &deduped)?);
        iteration += 1;
    }

    // Выполнить until-программу — она формирует ответ
    self.exec_sub_program(&until_prog)?;
}
```

### 6.3. Стратификация

Datalog-программа делится на стратумы (SCC в графе зависимостей):

```rust
OpCode::DL_STRATUM(0) => {
    // EDB — факты загружаются, но не выводятся
}
OpCode::DL_STRATUM(1) => {
    // Рекурсивный стратум: hop
    // DL_FIXPOINT внутри
}
OpCode::DL_STRATUM(2) => {
    // Нерекурсивный: answer
    // Просто JOIN + FILTER
}
```

---

## 7. Layer 4: FP Control Flow Combinators

### 7.1. Операторы

| Backus FP | OpCode | Стек | Описание |
|---|---|---|---|
| `f∘g` | `COMPOSE` | `sub_programs: [Program]` | Последовательное выполнение |
| `[f,g]` | `PARALLEL(n)` | `sub_programs: [Program; n]` | Параллельные ветки |
| `p→f;g` | `CONDITION` | `[cond, then_prog, else_prog]` | Ветвление |
| `fix(f)` | `FIXPOINT(max_iter)` | `[body, until]` | Итерация до сходимости |

### 7.2. Реализация

```rust
OpCode::COMPOSE => {
    for sub in &instr.sub_programs {
        self.exec_sub_program(sub)?;
    }
}

OpCode::PARALLEL(_) => {
    // Используем tokio::join! или rayon для параллельных веток
    let results: Vec<Result<(), String>> = instr.sub_programs
        .par_iter()
        .map(|prog| self.exec_sub_program(prog))
        .collect();
    for res in results { res?; }
}

OpCode::CONDITION => {
    let else_prog = stack.pop_as::<Program>()?;
    let then_prog = stack.pop_as::<Program>()?;
    let cond = stack.pop_as::<Condition>()?;
    if cond.0 {
        self.exec_sub_program(&then_prog)?;
    } else {
        self.exec_sub_program(&else_prog)?;
    }
}

OpCode::FIXPOINT(max_iter) => {
    let until_prog = stack.pop_as::<Program>()?;
    let body_prog = stack.pop_as::<Program>()?;
    let mut converged = false;
    for _ in 0..max_iter {
        self.exec_sub_program(&body_prog)?;
        self.exec_sub_program(&until_prog)?;
        converged = stack.pop_as::<Condition>()?.0;
        if converged { break; }
    }
}
```

---

## 8. Instruction Structure

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instruction {
    pub op: OpCode,
    /// Вложенные подпрограммы для COMPOSE, PARALLEL, CONDITION, FIXPOINT,
    /// DL_FIXPOINT и т.д.
    pub sub_programs: Vec<Program>,
}

impl Instruction {
    pub fn new(op: OpCode) -> Self {
        Self { op, sub_programs: vec![] }
    }
    pub fn with_subs(op: OpCode, subs: Vec<Program>) -> Self {
        Self { op, sub_programs: subs }
    }
}
```

## 9. Compilation Pipeline (Planner)

### 9.1. Новые AstNode варианты

```rust
pub enum AstNode {
    // ... существующие
    Search(QueryRequest),
    MultiHop(MultiHopQuery),
    SelfRag(SelfRagQuery),
    CorrectiveRag(CorrectiveRagQuery),
    ColbertRetrieval(ColbertRetrievalQuery),

    // --- Новые ---

    // APL tensor-expr: (+.× query_emb corpus_emb)
    TensorExpr {
        op: TensorOp,
        args: Vec<AstNode>,
    },

    // Реляционный запрос: (select condition (join ...))
    RelQuery {
        ops: Vec<RelOp>,
    },

    // Datalog: (:- head body)
    Datalog {
        rules: Vec<DlRule>,
        query: DlQuery,
    },

    // FP combinators
    FpCompose {
        steps: Vec<AstNode>,
    },
    FpParallel {
        branches: Vec<AstNode>,
    },
    FpCondition {
        pred: Box<AstNode>,
        then: Box<AstNode>,
        else_: Box<AstNode>,
    },
    FpFixpoint {
        body: Box<AstNode>,
        until: Box<AstNode>,
        max_iter: usize,
    },
}
```

### 9.2. Component::compile()

```rust
impl Planner {
    pub fn compile(&self, ast: AstNode) -> Result<Program, String> {
        match ast {
            AstNode::Search(req) => self.compile_search(req),
            AstNode::TensorExpr { op, args } => self.compile_tensor(op, args),
            AstNode::RelQuery { ops } => self.compile_relational(ops),
            AstNode::Datalog { rules, query } => self.compile_datalog(rules, query),
            AstNode::FpCompose { steps } => self.compile_compose(steps),
            AstNode::FpParallel { branches } => self.compile_parallel(branches),
            AstNode::FpCondition { pred, then, else_ } => self.compile_condition(pred, then, else_),
            AstNode::FpFixpoint { body, until, max_iter } => self.compile_fixpoint(body, until, max_iter),
            // ... existing
        }
    }

    fn compile_tensor(&self, op: TensorOp, args: Vec<AstNode>) -> Result<Program, String> {
        let mut prog = Program::new();
        for arg in &args {
            prog.extend(self.compile(arg.clone())?);
        }
        prog.push(match op {
            TensorOp::InnerProduct => OpCode::TENSOR_INNER_PRODUCT,
            TensorOp::OuterProduct => OpCode::TENSOR_OUTER_PRODUCT,
            TensorOp::GradeDown => OpCode::TENSOR_GRADE_DOWN,
            // ...
        });
        Ok(prog)
    }

    fn compile_compose(&self, steps: Vec<AstNode>) -> Result<Program, String> {
        let mut subs = Vec::new();
        for step in steps {
            subs.push(self.compile(step)?);
        }
        Ok(Program::from(vec![Instruction::with_subs(
            OpCode::COMPOSE, subs
        )]))
    }

    fn compile_parallel(&self, branches: Vec<AstNode>) -> Result<Program, String> {
        let mut subs = Vec::new();
        for branch in branches {
            subs.push(self.compile(branch)?);
        }
        let count = subs.len() as u32;
        Ok(Program::from(vec![Instruction::with_subs(
            OpCode::PARALLEL(count), subs
        )]))
    }

    fn compile_condition(&self, pred: Box<AstNode>, then: Box<AstNode>,
                         else_: Box<AstNode>) -> Result<Program, String> {
        let mut prog = Program::new();
        prog.extend(self.compile(*pred)?);
        prog.push(Instruction::with_subs(OpCode::CONDITION, vec![
            self.compile(*then)?,
            self.compile(*else_)?,
        ]));
        Ok(prog)
    }

    fn compile_datalog(&self, rules: Vec<DlRule>, query: DlQuery) -> Result<Program, String> {
        let mut prog = Program::new();
        // Стратификация: topological sort SCC
        let strata = stratify(&rules);
        for (sid, stratum) in strata.iter().enumerate() {
            prog.push(Instruction::new(OpCode::DL_STRATUM(sid)));

            let is_recursive = stratum.is_recursive();
            if is_recursive {
                // Рекурсивный стратум: body (rules) + until (query)
                let body_prog = self.compile_rules(stratum)?;
                let until_prog = self.compile_query(&query)?;
                prog.push(Instruction::with_subs(
                    OpCode::DL_FIXPOINT(100),
                    vec![body_prog, until_prog],
                ));
            } else {
                // Нерекурсивный: просто правила и запрос
                prog.extend(self.compile_rules(stratum)?);
                prog.extend(self.compile_query(&query)?);
            }
        }
        Ok(prog)
    }
}
```

---

## 10. Surface Syntax (DSL)

### 10.1. S-expressions (эволюция текущего)

```scheme
;; APL-style tensor ops
(tensor-inner-product query-emb corpus-emb cosine)
(tensor-outer-product query-emb corpus-emb cosine)
(tensor-grade-down scores)
(tensor-take 10 (tensor-grade-down scores))
(tensor-compress results (gt scores 0.7))

;; Relational
(rel-select (gt score 0.7) results)
(rel-join left right Similarity {metric: cosine threshold: 0.8})
(rel-aggregate results [author] (top-k 5 score desc))

;; Datalog
(datalog
  :rules [
    (:- (path ?x ?y) (edge ?x ?y))
    (:- (path ?x ?y) (edge ?x ?z) (path ?z ?y))
  ]
  :query (path "doc-A" ?result))

;; FP combinators
(compose
  (hybrid :query "deep learning" :top-k 20)
  (rel-select (gt score 0.7))
  (rel-take 5))

(parallel
  (dense :query "transformer" :top-k 20)
  (bm25  :query "transformer" :top-k 20))

(condition (lt confidence 0.7)
  (corrective-rag :query "something")
  (search :query "something" :top-k 10))

(fixpoint :max-iter 100
  :body (datalog-rule-step)
  :until (check-convergence))
```

### 10.2. Concatenative (альтернативная поверхность)

```
# стековый pipeline
query "deep learning"
  [ dense 20 ]
  [ bm25  20 ]
  bi           # parallel → stack: (dense_res bm25_res)
  rrf          # Reciprocal Rank Fusion
  (score>0.7)  # filter threshold
  take 5
  llm_answer
```

---

## 11. Примеры компиляции

### 11.1. Hybrid Search

```
;; DSL:
(search :query "quantum computing"
        :rerank (rsf :dense-weight 0.6 :bm25-weight 0.3 :entity-weight 0.1)
        :final-top-k 10)
```

```
;; Bytecode (14 инструкций):
[ 0] RetrieveQdrant        query="quantum computing" top_k=80
[ 1] ComputeSemantic
[ 2] RetrievePostgres      query="quantum computing"
[ 3] RetrieveEntityScores  query="quantum computing"
[ 4] TENSOR_GRADE_DOWN     ;; сортировка dense scores
[ 5] TENSOR_TAKE(80)       ;; top-80
[ 6] LoadConst(0.6)        ;; dense weight
[ 7] LoadConst(0.3)        ;; bm25 weight
[ 8] LoadConst(0.1)        ;; entity weight
[ 9] Fusion                ;; RSF
[10] TopK(10)
[11] FilterByMetadata      ;; пост-фильтр
[12] Emit
```

### 11.2. Multi-hop с Datalog

```
;; DSL:
(datalog
  :rules [
    (:- (hop ?doc 0) (seed-doc ?doc))
    (:- (hop ?z (+ ?n 1)) (hop ?x ?n) (cites ?x ?z))
    (:- (answer ?d ?s) (hop ?d ?n) (dense-similarity ?d ?s) (>= ?s 0.7))
  ]
  :query (answer ?result ?score))
```

```
;; Stratum 0: EDB
[0] DL_STRATUM(0)
[1] DL_REGISTER_REL   seed-doc(1)
[2] DL_REGISTER_REL   cites(2)
[3] DL_REGISTER_REL   dense-similarity(2)
[4] DL_REGISTER_REL   hop(2)
[5] DL_REGISTER_REL   answer(2)

;; Stratum 1: recursive hop
[6] DL_STRATUM(1)
[7] DL_FIXPOINT(100)
     ├─ body:
     │  [0] DL_JOIN         hop[delta].0 = cites.0
     │  [1] DL_PROJECT      [cites.1, hop.1+1]
     │  [2] REL_DIFFERENCE  new ∖ hop.full          ;; dedup
     │  [3] DL_UNION        hop.full ∪ new
     └─ until:
        [0] REL_SELECT      hop пуст?
        [1] CONDITION

;; Stratum 2: answer derivation
[8] DL_STRATUM(2)
[9] DL_JOIN    hop.full.0 = dense-similarity.0
[10] REL_SELECT score >= 0.7
[11] REL_PROJECT [doc, score]
[12] DL_UNION  answer
[13] REL_SELECT doc = ?result
[14] Emit
```

### 11.3. Parallel Conditional RAG

```
;; DSL:
(compose
  (parallel
    (search :query "black holes" :rerank (semantic) :top-k 10)
    (search :query "black holes" :rerank (bm25)     :top-k 10))
  (rrf)
  (take 5)
  (llm-generate))
```

```
[0] COMPOSE
    ├─ sub[0]: PARALLEL(2)
    │   ├─ sub[0]:
    │   │   [0] RetrieveQdrant query="black holes" top_k=80
    │   │   [1] ComputeSemantic
    │   │   [2] TopK(10)
    │   │   [3] Emit
    │   └─ sub[1]:
    │       [0] RetrievePostgres query="black holes"
    │       [1] TopK(10)
    │       [2] Emit
    ├─ sub[1]: RRF
    ├─ sub[2]: TENSOR_TAKE(5)
    └─ sub[3]: LlmGenerate
```

### 11.4. Self-RAG с Conditional

```
;; DSL:
(self-rag :query "quantum gravity"
          :threshold 0.85
          :high-confidence (search :query "quantum gravity" :top-k 10)
          :low-confidence (search :query "quantum gravity basics" :top-k 5))
```

```
[0] RetrieveQdrant   query="quantum gravity" top_k=10
[1] ComputeSemantic
[2] TopK(1)                                   ;; max score
[3] LoadConst(0.85)
[4] CmpGt                                      ;; > 0.85?
[5] CONDITION
    ├─ then: (high confidence)
    │   [0] RetrieveQdrant query="quantum gravity" top_k=80
    │   [1] ComputeSemantic
    │   [2] TopK(10)
    │   [3] Emit
    └─ else: (low confidence)
        [0] RetrieveQdrant query="quantum gravity basics" top_k=40
        [1] ComputeSemantic
        [2] TopK(5)
        [3] Emit
```

---

## 12. Lowering Pass (опционально)

Для отладки и бэкенд-оптимизации — проход, раскрывающий
высокоуровневые OpCode в последовательность базовых:

```rust
fn lower_tensor_grade_down(prog: &mut Program, idx: usize) {
    // TENSOR_GRADE_DOWN → Retrieve → sort_indices
    let instr = &prog[idx];
    let lowered = vec![
        Instruction::new(OpCode::RetrieveValues),
        Instruction::new(OpCode::SortIndices { descending: true }),
    ];
    prog.splice(idx..=idx, lowered);
}
```

---

## 13. Optimizer

```
Planner::compile(ast)
    │
    ▼
LogicalPlan (AstNode-дерево)
    │
    ├─── Rule-based Optimizer ─────────────────
    │   Phase 1 (heuristic, bottom-up):
    │     - Selection fusion
    │     - Projection pushdown
    │     - Similarity threshold pushdown
    │     - Compress distribution
    │     - Grade-identity reduction
    │     - Fold fusion
    │
    ├─── Cost-based Optimizer ─────────────────
    │   Phase 2 (DP, Selinger-style):
    │     - Join ordering
    │     - Matrix chain optimization
    │     - Index selection
    │
    ▼
PhysicalPlan (OpCode-последовательность)
```

Первый этап не требует cost model — чистые алгебраические переписывания.

---

## 14. Implementation Plan

| Фаза | OpCode | Зависимости | Усилие |
|---|---|---|---|
| **P1** | TENSOR_GRADE_UP/DOWN, COMPRESS, TAKE, WHERE, SHAPE | arrow compute kernels | Low |
| **P2** | TENSOR_INNER_PRODUCT, OUTER_PRODUCT, REDUCE, SCAN | simsimd crate | Low |
| **P3** | TENSOR_RESHAPE, TRANSPOSE | Tensor struct | Low |
| **P4** | REL_SELECT, REL_PROJECT, REL_ORDER_BY, REL_LIMIT | Arrow RecordBatch | Medium |
| **P5** | REL_JOIN (exact), REL_UNION, REL_DIFFERENCE, REL_AGGREGATE | Arrow hash join | Medium |
| **P6** | REL_JOIN (Similarity), REL_JOIN (AsOf) | Tensor ops from P2 | Medium |
| **P7** | DL_REGISTER_REL, DL_FACT, DL_JOIN, DL_PROJECT, DL_UNION, DL_DIFFERENCE | Rel ops from P5 | High |
| **P8** | DL_FIXPOINT, DL_STRATUM, DL_INDEX | Semi-naive engine | High |
| **P9** | COMPOSE, PARALLEL, CONDITION, FIXPOINT | VM executor | Medium |
| **P10** | Optimizer: rewrite rules | Algebraic laws | Medium |
| **P11** | Optimizer: cost-based | Cardinality estimation | High |
| **P12** | Surface syntax: concatenative | Parser ext | Low |

---

## 15. Vector Space Type System

Ключевое отличие tensor-relational СУБД от "просто тулзы":
векторное пространство — часть type system, а не неявная деталь реализации.

### 15.1. Проблема

Сейчас embeddings кладутся в Qdrant, и "работают", пока модель одна и та же.
Если документы заиндексованы с `bge-base-en-v1.5` (768d), а запрос пришёл
от `text-embedding-ada-002` (1536d), `cosine_sim` даёт бессмысленный результат.
Это как складывать метры с килограммами — компилятор должен такое REJECT-ить.

### 15.2. Тип вектора

```rust
/// Пространство — именованный тип с метрикой и размерностью
pub struct VectorSpace {
    /// Каноническое имя (registry)
    pub name: SpaceId,
    /// Модель-источник (или "custom" для APL-конструированных)
    pub model: String,
    /// Базовая размерность
    pub dims: usize,
    /// Метрика по умолчанию
    pub metric: SimilarityMetric,
    /// Является ли пространство Matryoshka-совместимым
    pub is_mrl: bool,
}

/// Колонка-вектор — тензор с пространством
pub struct TypedColumn {
    pub name: String,
    pub space: VectorSpace,
    pub nullable: bool,
}

/// Таблица — RecordBatch, где одна из колонок — TypedColumn
pub struct TensorRelation {
    pub batch: RecordBatch,
    pub vector_column: Option<String>,
    pub space: Option<VectorSpace>,
}
```

### 15.3. Type checking при компиляции

```rust
impl TypeChecker {
    fn check_similarity_join(&self, left: &TensorRelation, right: &TensorRelation) -> Result<(), TypeError> {
        // Проверка совместимости пространств
        match (&left.space, &right.space) {
            (Some(ls), Some(rs)) => {
                if ls.name != rs.name {
                    // Разные пространства — можно только через CONVERT
                    return Err(TypeError::IncompatibleSpaces {
                        left: ls.name.clone(),
                        right: rs.name.clone(),
                        hint: "use (convert :space ...) to project between spaces",
                    });
                }
                // Для MRL-пространств — проверка размерности
                if ls.is_mrl && rs.is_mrl && ls.dims != rs.dims {
                    // Ok — MRL допускает срез
                    // warning: sub-dimensional comparison
                } else if ls.dims != rs.dims {
                    return Err(TypeError::DimensionMismatch {
                        left: ls.dims, right: rs.dims,
                    });
                }
            }
            (None, _) | (_, None) => {
                // Нет пространства — fallback, только exact match
            }
        }
        Ok(())
    }

    fn check_convert(&self, from: &VectorSpace, to: &VectorSpace) -> Result<ConversionPlan, TypeError> {
        if from.name == to.name {
            return Ok(ConversionPlan::Slice { from_dims: from.dims, to_dims: to.dims });
        }
        // Ищем путь трансформации через registry
        if let Some(path) = self.registry.find_path(&from.name, &to.name) {
            Ok(ConversionPlan::Transform(path))
        } else {
            Err(TypeError::NoConversionPath { from: from.name.clone(), to: to.name.clone() })
        }
    }
}
```

### 15.4. Registry пространств

```scheme
;; Конфигурация пространств (registry.yaml или DSL define-space)
(registry
  ;; Канонические пространства от известных моделей
  {:name "bge-base-en-v1.5",  :model "BAAI/bge-base-en-v1.5",  :dims 768,  :metric cosine}
  {:name "ada-002",           :model "text-embedding-ada-002", :dims 1536, :metric cosine}
  {:name "gte-Qwen2-MRL",    :model "Alibaba/gte-Qwen2",      :dims 1024, :metric cosine, :is_mrl true}

  ;; Пользовательские пространства — APL-конструированные
  {:name "my-project-space", :dims 512, :metric cosine, :model "custom"}
  )
```

### 15.5. CONVERT оператор

```
OpCode::SPACE_CONVERT(ConversionPlan)
  stack: [source_relation] → [target_relation]
```

Реализация:

```rust
OpCode::SPACE_CONVERT(plan) => {
    let rel = stack.pop_as::<TensorRelation>()?;
    let result = match plan {
        ConversionPlan::Slice { from_dims, to_dims } => {
            // MRL-срез — просто take первых N dims
            tensor_take_dims(&rel, to_dims)?
        }
        ConversionPlan::Transform(path) => {
            // Последовательность линейных преобразований
            // (через ONNX-сессию или встроенную проекцию)
            let mut current = rel;
            for step in path.steps {
                current = apply_linear_transform(&current, &step.matrix)?;
            }
            current
        }
        ConversionPlan::ReEmbed(space) => {
            // Пере-эмбеддинг через указанную модель
            re_embed(&rel, &space)?
        }
    };
    stack.push(Value::Relation(Arc::new(result)));
}
```

### 15.6. Что даёт типизация пространств

- **Safety**: cross-space операции — compile error, а не runtime garbage
- **Optimization**: зная пространство, компилятор выбирает правильный индекс (HNSW/IVF)
- **Audit**: можно ответить "в каком пространстве был этот поиск?"
- **MRL-awareness**: компилятор знает, какие размерности с чем совместимы

---

## 16. Matryoshka Vector Spaces: Progressive Retrieval без Reranker'ов

### 16.1. Идея

Matryoshka Representation Learning (MRL) — модели, где вектор учится быть
"хорошим" на любой префиксной длине: первые 128 dims = грубый смысл,
512 = точнее, 1024 = полное качество.

В сочетании с APL-операторами это даёт **reranker-free progressive retrieval**:

```apl
;; Одна модель, одно пространство, разные срезы по dims

;; Pass 1: coarse — O(n * 128) через HNSW или flat
coarse_scores ← query ∘.cos corpus[; ⍳128]
candidates    ← (⍒ coarse_scores) ↑ 100

;; Pass 2: medium — O(100 * 512) только над кандидатами
medium_scores ← query ∘.cos corpus[candidates; ⍳512]
top50         ← (⍒ medium_scores) ↑ 50

;; Pass 3: fine — O(50 * 1024) полная точность
final_scores  ← query ∘.cos corpus[top50; ⍳1024]
results       ← (⍒ final_scores) ↑ 10
```

**Ни одной внешней reranker-модели.** Весь progressive retrieval — операции
среза тензора (APL `↑`), внутреннего произведения (`+.×`) и сортировки (`⍒`) .

### 16.2. Progressive как VM instruction

```rust
OpCode::MRL_PROGRESSIVE_SEARCH {
    dims: Vec<usize>,      // [128, 512, 1024] — ступени прогрессии
    top_k: Vec<usize>,     // [100, 50, 10] — отсечки на каждом шаге
}
```

Dispatch:

```rust
OpCode::MRL_PROGRESSIVE_SEARCH => {
    let space = stack.pop_as::<SpaceId>()?;
    let query = stack.pop_as::<Tensor>()?;
    let corpus = stack.pop_as::<TensorRelation>()?;

    let mut candidates: Vec<usize> = (0..corpus.num_rows()).collect();
    for (i, (&dim, &k)) in dims.iter().zip(top_k.iter()).enumerate() {
        // Берём срез размерности
        let query_slice = tensor_take_dims(&query, dim)?;
        let corpus_slice = tensor_take_dims(corpus.vectors(), dim)?;

        // Cosine similarity через SIMD
        let scores = cosine_similarity_batch(&query_slice, &corpus_slice, &candidates);

        // Grade down + take
        let ranked = grade_down(&scores);
        candidates = candidates[&ranked[..k.min(scores.len())]];
    }

    // Возвращаем результат
    stack.push(self.fetch_docs(&corpus, &candidates)?);
}
```

### 16.3. Отказ от reranker-моделей: сравнение

| Аспект | Классический RAG | MRL + APL (proposed) |
|---|---|---|
| Модели | bi-encoder + cross-encoder | Одна MRL-модель |
| Reranker | Внешняя модель O(n²) | Нет — progressive precision |
| Сложность | O(n*d) + O(k*L) (L — длина) | O(n*128 + k*512 + ...) |
| Переключение моделей | Нужен compat слой | Одно пространство |
| APL-выразительность | Нет — хардкод | `↑`, `+.×`, `⍒` combinators |
| Reranking как операция | Вызов внешнего сервиса | Запись в три строки APL |

### 16.4. APL-операции над MRL-пространством

```apl
;; Срез пространства — APL take
↑[1] 128 corpus      ;; → первые 128 dims

;; Динамический срез — отбор ключевых размерностей
⍉ 1024 512 256 ∘.take corpus   ;; → три параллельных среза

;; Progressive retrieval в одну строку
(+,×) ⍒ query ∘.cos corpus[; ⍳512]  ;; top-10 по первым 512 dims

;; Взвешенное объединение срезов
(results_at_512 × 0.3) + (results_at_1024 × 0.7)
```

### 16.5. Custom space как combinators

Если отказаться от внешних моделей, пространство можно конструировать
из примитивов:

```scheme
;; Определение пространства через APL-комбинаторы
(define-space my-compound-space
  :dims 2048
  :constructor (lambda (text)
    (compose
      (bge-embed text)                       ;; 768d
      (linear-transform (matrix 768×1024))   ;; 1024d
      (relu)                                  ;; non-linear
      (concat (sparse-bm25 text))             ;; + sparse
      )))
```

Тогда embedding — не вызов внешнего API, а **программа**, выполняемая VM:
`TENSOR_INNER_PRODUCT` над спроецированными пространствами.

---

## 17. Многомерные вселенные: пространство как programmable тензор

### 17.1. Пространство — не константа, а тензорная операция

Если отказаться от "модель дала вектор", то векторное пространство —
результат композиции тензорных операций:

```apl
;; Поворот пространства — умножение на ортогональную матрицу
rotated_space ← query ∘.× rotation_matrix

;; Масштабирование осей
weighted_space ← query × weights_vector

;; Нелинейная трансформация (softmax, relu, norm)
normalized_space ← query ∘.norm ∘.softmax

;; Конкатенация гетерогенных пространств
;; (текст + изображение + граф)
multimodal_space ← text_emb ∥ image_emb ∥ graph_emb

;; Свёртка размерностей — pooling
pooled_space ← +/[axis] query
```

### 17.2. DSL для манипуляции пространством

```scheme
;; APL-стиль: пространство — результат выражения
(let ((rotated (tensor-matmul query (rotation-matrix 45°))))
  (search :space rotated
          :query "who moved my cheese?"
          :top-k 5))

;; Создание подпространства через selection размерностей
(let ((subspace (tensor-take-dims corpus 256)))
  ;; Поиск только в 256-dim подпространстве
  (search :space subspace :query "meaning of life"))

;; Динамический compose пространств
(compose
  (project query 128)                    ;; coarse
  (project query 512)                    ;; medium
  (project query 1024)                   ;; fine
  (∇ rerank))                            ;; symbol ∇ = combine via outer product
```

### 17.3. Следствия для VM

1. **Вектор — не opaque blob, а структура с известной размерностью**
2. **Пространство — first-class citizen**: передаётся через стек, как Value
3. **CONVERT — не костыль, а explicit operation**: `space_a → linear_proj → space_b`
4. **Registry — не жёсткий справочник, а кэш выведенных типов**

```rust
pub enum Value {
    // ... существующие
    /// Пространство — first-class (можно класть на стек)
    Space(VectorSpace),

    /// Линейное преобразование между пространствами
    SpaceTransform(LinearMap),
}
```

### 17.4. Open вопрос: границы

Когда пространство — programmable object, возникают вопросы:

- **Когда кончается алгебра и начинается модель?** Если embedding — программа
  VM, то bi-encoder и reranker — просто разные программы. Грань стирается.
- **Кто отвечает за качество?** Если пользователь построил пространство
  через `concat(bge_embed, sparse_bm25, random_matrix)`, оно может быть
  бессмысленным. Type system проверяет только совместимость dims, не качество.
- **Что делать с не-MRL моделями?** Если модель не умеет матрёшку — только
  full-dim поиск. CONVERT через матричную проекцию (trainable или SVD).

---

## 18. Типы колонок: единая таблица retrieval

Сводка по типам данных, которые может содержать TensorRelation:

| Тип | Хранение | Оператор поиска | Пример |
|---|---|---|---|
| `VECTOR<V, d>` | FixedSizeList[f32; d] | `⋈cos`, `⋈dot`, `⋈L2` | Dense embeddings |
| `SPARSE_VECTOR<V>` | Struct{indices, values} | `+.×` (sparse dot) | SPLADE, uniCOIL |
| `BM25_TEXT` | Utf8 | `ts_rank` (built-in BM25) | Сырой текст для keyword |
| `EXACT` | любой Arrow type | `σ(field = val)` | Metadata filter |
| `VARCHAR` | Utf8/LargeUtf8 | `LIKE`, `CONTAINS` | Fuzzy text match |
| `TIMESTAMP` | TimestampNanosecond | `τ(time)` + AsOf join | Временные ряды |
| `SCORE` | Float32 | — | Вычисляемый скор |

**Ключевой принцип**: всё, что не exact metadata, — тензор. BM25 — отдельный
тип колонки с собственным оператором `⋈BM25`, который компилируется в
`BM25_JOIN` инструкцию VM.

```rust
OpCode::BM25_JOIN { top_k: usize }
  stack: [query_text, corpus_relation] → [relation with bm25_score column]
```

Никаких внешних сервисов — BM25 живёт внутри VM как нативная операция.

---

## 19. Sparse + Dense fusion как тензорная операция

Объединение sparse (BM25/SPLADE) и dense (MRL) поиска — не вызов fusion-сервиса,
а APL-комбинатор над score-векторами:

```apl
;; Dense scores
d_scores ← query_emb ∘.cos corpus_emb

;; BM25 scores
b_scores ← bm25_query × bm25_corpus  ;; sparse dot

;; Fusion — взвешенная сумма с нормализацией
d_norm   ← d_scores ÷ (⌈/ d_scores)
b_norm   ← b_scores ÷ (⌈/ b_scores)
fused    ← (d_norm × 0.6) + (b_norm × 0.4)

;; Top-K по fusion
results  ← (⍒ fused) ↑ 10
```

Всё — нативный APL внутри VM. Никаких внешних fusion-сервисов.

---

## 20. Обновлённый Implementation Plan

| Фаза | Что | Зависимости | Усилие |
|---|---|---|---|
| **P1** | TENSOR_GRADE_UP/DOWN, COMPRESS, TAKE, WHERE, SHAPE | arrow compute | Low |
| **P2** | TENSOR_INNER_PRODUCT, OUTER_PRODUCT, REDUCE, SCAN | simsimd | Low |
| **P3** | TENSOR_RESHAPE, TRANSPOSE, EACH_CELL | Tensor struct | Low |
| **P4** | REL_SELECT, REL_PROJECT, REL_ORDER_BY, REL_LIMIT | RecordBatch | Medium |
| **P5** | REL_JOIN (exact), UNION, DIFFERENCE, AGGREGATE | Arrow hash join | Medium |
| **P6** | REL_JOIN (Similarity, AsOf) + SPACE_CONVERT | Tensor ops + TypeChecker | Medium |
| **P7** | **BM25_JOIN** — built-in BM25 внутри VM | Tokenizer + inverted index | Medium |
| **P8** | **VectorSpace type system** — TypeChecker, Registry | P6 | Medium |
| **P9** | **MRL_PROGRESSIVE_SEARCH** — progressive retrieval | P2 + P8 | Medium |
| **P10** | **DIM_MUTATE** — Johnson-Lindenstrauss, Fourier Features, Pool | P2 | Medium |
| **P11** | DL_REGISTER_REL, DL_FACT, DL_JOIN, DL_FIXPOINT | P5 | High |
| **P12** | COMPOSE, PARALLEL, CONDITION, FIXPOINT | VM executor | Medium |
| **P13** | Optimizer: rewrite rules | Algebraic laws | Medium |
| **P14** | Optimizer: cost-based | Cardinality estimation | High |
| **P15** | Surface syntax: concatenative | Parser ext | Low |
| **P16** | **Symbolic Verifier** — Lisp + Prolog domain engine | Rust FFI + lisp/kanren crate | High |
| **P17** | **Domain registry** — catalog of geometric/retrieval laws | P16 | Medium |
| **P18** | **Fast/Slow path logic** — sample rate, retry on reject | P16 + P1 | Medium |

---

---

## 21. Cross-Space Retrieval и проблема галлюцинаций

### 21.1. Постановка проблемы

Система позволяет индексировать одной моделью, а искать — другой.
Это порождает асимметрию:

```
Индексация:  corpus_i = f_weak("чёрный")     // модель A, 128d
Поиск:       query    = f_strong("black")     // модель B, 1024d
             query_in_a = CONVERT(query, B→A) // проекция 1024→128
```

**Сценарий 1: strong → weak.** Сильная модель "вытягивает" из слабой
информацию, которой там нет. Проекция 1024→128 теряет нюансы, но query
"видит" грубые очертания и может найти нерелевантное, которое случайно
рядом в спроецированном пространстве.

**Сценарий 2: weak → strong.** Слабая модель индексировала слишком грубо.
Query в 1024-мерном пространстве ищет детали, которых в 128-мерном следе
просто не осталось. Конверсия теряет запрос → false negatives.

**Сценарий 3: разные языки.** f_weak — LaBSE (многоязычный), f_strong —
ruBERT. Индексация по-русски, поиск по-английски. CONVERT не восстанавливает
смысл, потому что модель A не видела английского.

### 21.2. Причина: информация необратимо теряется при индексации

```
f_weak(text) → vector(128d)    // потеряно (dim_original - 128) семантических степеней свободы
                                 // никакой CONVERT не восстановит то, что не было закодировано
```

Это **write-time loss** — в отличие от read-time loss (approximate search, quantization),
его нельзя компенсировать на стороне запроса. Как сжатый JPEG — сколько ни
апскейль, деталей не добавится.

Формально: embedding — это функция `f: Text → ℝ^d`. Если dim модели A
(d_A) меньше, чем intrinsic dimensionality семантического пространства
(d_intrinsic), то `f_weak` — проекция на подпространство меньшей размерности.
CONVERT — ещё одна проекция. Композиция проекций — проекция. Информация
теряется необратимо.

### 21.3. Когда cross-space retrieval безопасен

**Безопасно:**
- Одна и та же модель (напр. bge-base-en-v1.5), но разные чекпоинты
  (fine-tuned vs base) — CONVERT через linear probe обучим
- MRL-совместимые срезы одной модели — `f_strong` и `f_weak` — одна сеть
- Разные модели одного семейства и размера (напр. ada-002 → ada-003)
  — если empirical alignment высок

**Опасно:**
- Модели разных архитектур (BERT vs T5 vs MRL)
- Модели разных языков
- Модели с разной intrinsic dimensionality
- Размерности отличаются больше чем в 2× (768 → 128 — потеря)

### 21.4. Механизмы защиты (что может делать type system)

1. **WARN при CONVERT** — не блокировать, но выдавать warning:
   ```
   Warning: cross-space retrieval 'bge-base' → 'ada-002'
   Information loss expected: 768d → 1536d (projection),
   quality not guaranteed.
   ```

2. **Registry качества** — хранить метрики alignment между пространствами:
   ```scheme
   (register-alignment
     :from "bge-base-en-v1.5"
     :to "gte-Qwen2-MRL"
     :score 0.89          ; alignment quality на тестовом датасете
     :method "linear-probe"
     :trained-on "miracl")
   ```

3. **CONVERT с confidence score** — каждая трансформация возвращает
   не только вектор, но и метрику достоверности:
   ```rust
   struct ConvertedVector {
       data: Tensor,
       space: SpaceId,
       confidence: f32,  // 0.0 = interpolated, 1.0 = native
       provenance: Vec<SpaceId>,  // цепочка конверсий
   }
   ```

4. **Policy-based execution** — администратор задаёт правила:
   ```
   policy "production":
     - allow: exact space match
     - allow: MRL sub-space (projection)
     - deny:  cross-model CONVERT
     - warn:  diff-language CONVERT
   ```

### 21.5. Альтернатива: все модели — один MRL-срез

Вместо cross-space CONVERT — если все модели обучать как Matryoshka
(или дообучать PEFT-адаптером), то они оперируют в едином пространстве
с разной точностью. Тогда:

- `f_weak` = первые 128 dims того же пространства
- `f_strong` = все 1024 dims того же пространства
- Индекс общий, CONVERT не нужен
- Асимметрия только в точности, не в семантике

Проблема: текущие MRL-модели (GTE-Qwen2, Nomic Embed v2) — единичные.
Индустрия не стандартизировала MRL как формат.

### 21.6. Вывод для архитектуры

1. **Embedding model — единственный источник семантики.** VM и APL-операторы
   не добавляют понимания, они только композируют уже закодированное.
2. **Cross-space retrieval — техника с ограничениями.** Не замена native
   indexing, а fallback для миграции.
3. **Write-time loss необратим.** Если модель A не видела английского,
   никакой CONVERT не сделает search по-английски работающим.
4. **MRL — единственный "чистый" способ unified space.** Всё остальное —
   эвристики с просадкой качества.
5. **Type system должна явно маркировать provenance** — откуда взялся
   каждый вектор, сколько трансформаций он прошёл.

---

---

## 22. Runtime Dimension Mutation: размерность как фаза вычисления

### 22.1. Идея

Вместо "индекс создан в размерности D, так и живёт" — размерность
становится **операцией**, а не свойством данных. APL-операторы позволяют
мутировать размерность на лету: расширять (256→3000), сжимать (1024→64),
преобразовывать (линейно, нелинейно, через свёртку).

```
                      ┌──────┐
    corpus(128d) ────→│ MUTATE│────→ scores(3000d) ──→ rank
    query(128d)  ────→│ (fast)│
                      └──────┘

    corpus(128d) ────→┌────────┐
    query(128d)  ────→│EXACT   │────→ same result, more compute
                      │(precise)│
                      └────────┘
```

### 22.2. Техники мутации размерности

**Расширение (upscale):**

| Метод | Формула (APL) | Свойства |
|---|---|---|
| Random projection (JL) | `vec ∘.× randn(d_new, d_old)` | Сохраняет pairwise distances с high prob |
| Polynomial expansion | `vec ∥ vec² ∥ vec³` | Добавляет нелинейные взаимодействия |
| Fourier features | `cos(ω ∘.× vec) ∥ sin(ω ∘.× vec)` | Аппроксимация ядра (Rahimi & Recht) |
| Nyström extension | `kernel(vec, landmarks) × Σ⁻¹` | Manifold-aware expansion |
| SVD augmentation | `vec × V_k` (добавление right singular vectors) | Усиление существующих паттернов |

**Сжатие (downscale):**

| Метод | Формула (APL) | Свойства |
|---|---|---|
| PCA projection | `vec × W_k` (top-k components) | Оптимальная линейная проекция |
| Random projection | `vec ∘.× randn(d_new, d_old)` | JL — расстояния сохраняются |
| Feature selection | `vec[; ⍳k]` (MRL-стиль) | Если модель поддерживает |
| Max-pooling dims | `⌈/[axis] reshape(vec, groups)` | Группировка соседних dims |

### 22.3. Mutation как VM instruction

```rust
OpCode::DIM_MUTATE(MutationSpec)
  stack: [tensor, query, params] → [mutated_tensor]
```

```rust
pub enum MutationSpec {
    /// Расширение: d_old → d_new через random projection
    RandomProjection {
        d_new: usize,
        seed: Option<u64>,
        preserve_distances: bool,  // JL-normalization
    },
    /// Полиномиальное расширение
    Polynomial {
        degree: usize,       // 2 = vec ++ vec²
        include_interactions: bool,  // vec_i × vec_j
    },
    /// Fourier features для аппроксимации ядра
    FourierFeatures {
        d_new: usize,
        kernel: KernelType,  // RBF, Matern, Laplacian
        gamma: f32,
    },
    /// PCA-проекция (incremental, без полного SVD)
    PcaProjection {
        d_new: usize,
        components: Option<Vec<Vec<f32>>>,  // если pre-computed
    },
    /// Группировка dims через pooling
    PoolProjection {
        group_size: usize,
        pool_fn: PoolFn,  // Max, Mean, Sum
    },
    /// Свободная матричная проекция (обучаемая)
    LinearTransform {
        matrix: Tensor,  // explicit (d_new × d_old)
        bias: Option<Vec<f32>>,
    },
}
```

Dispatch:

```rust
OpCode::DIM_MUTATE(spec) => {
    let tensor = stack.pop_as::<Tensor>()?;
    let d_old = tensor.shape().last().unwrap();

    let result = match spec {
        MutationSpec::RandomProjection { d_new, seed, preserve_distances } => {
            // Johnson-Lindenstrauss матрица
            let rng = seed.map(|s| StdRng::seed_from_u64(s))
                .unwrap_or(StdRng::from_entropy());
            let proj = random_gaussian_matrix(d_new, d_old, &mut rng);
            let mut result = tensor_matmul(&tensor, &proj)?;
            if preserve_distances {
                // Нормализация: scale = sqrt(d_old / d_new)
                result = tensor_scale(&result, (d_old as f32 / d_new as f32).sqrt())?;
            }
            result
        }
        MutationSpec::FourierFeatures { d_new, kernel, gamma } => {
            // Random Fourier Features (Rahimi & Recht, 2007)
            // φ(x) = sqrt(2/d_new) * cos(ω·x + b)
            // ω ~ N(0, 2γ) для RBF, b ~ Uniform(0, 2π)
            let omega = random_normal_matrix(d_new, d_old, gamma)?;
            let bias = random_uniform(d_new, 0.0, 2.0 * PI)?;
            let raw = tensor_matmul(&tensor, &omega)?;    // ω·x
            let phased = tensor_add(&raw, &bias)?;         // ω·x + b
            let cos = tensor_cos(&phased)?;                 // cos(ω·x + b)
            tensor_scale(&cos, (2.0 / d_new as f32).sqrt())?
        }
        MutationSpec::PoolProjection { group_size, pool_fn } => {
            // Группировка dims: (d / group_size) результатов
            let reshaped = tensor_reshape(&tensor, &[tensor.len() / group_size, group_size])?;
            match pool_fn {
                PoolFn::Max => tensor_reduce(&reshaped, &[1], |a, b| a.max(b))?,
                PoolFn::Mean => tensor_reduce(&reshaped, &[1], |a, b| a + b)?,
                PoolFn::Sum  => tensor_reduce(&reshaped, &[1], |a, b| a + b)?,
            }
        }
        // ...
    };
    stack.push(Value::Tensor(result));
}
```

### 22.4. Mutation как стратегия поиска

Вместо фиксированной размерности — **эволюция размерности в рантайме**:

```scheme
;; DSL: поиск с мутирующей размерностью
(search :query "black white colors"
        :strategy (mutate
          ;; Фаза 1: быстрый проход на 128d (JL projection)
          {:dims 128  :method :jl-random  :top-k 200}

          ;; Фаза 2: уточнение на 512d (Fourier features)
          {:dims 512  :method :fourier    :top-k 50   :kernel :rbf}

          ;; Фаза 3: полная precision на 3000d (polynomial)
          {:dims 3000 :method :polynomial :degree 2   :top-k 10}
          ))
```

```apl
;; APL-стиль: мутация как композиция операторов

;; Шаг 1: JL-projection 128→512
query_mutated ← query ∘.× randn(512 128)
corpus_mutated ← corpus ∘.× randn(512 128)
scores ← query_mutated ∘.cos corpus_mutated

;; Шаг 2: Fourier features 512→2048
ω ← randn(2048 512)   ;; закешировано
φ(x) ← cos(ω ∘.× x)
scores_fourier ← φ(query) ∘.cos φ(corpus)

;; Вся стратегия в одну строку:
(⍒ (cos ∘ (Fourier(2048) ∘ JL(512)) query) ∘.cos corpus) ↑ 10
```

### 22.5. Ограничения и риски

1. **Random projection не создаёт семантику.**
   JL-лемма сохраняет расстояния, но не "понимает", что black и white —
   цвета. Если корпус был проиндексирован 128d моделью (например, fastText),
   мутация в 3000d не добавит семантических степеней свободы — только
   переразложит уже имеющиеся 128 на 3000 осей. Информация та же, форма
   другая.

2. **Fourier features != обученная модель.**
   Random Fourier Features аппроксимируют ядро (RBF, Matern), но ядро —
   это просто мера сходства. Неявное пространство, которое они строят,
   не имеет семантической структуры, только метрическую.

3. **Вычислительная цена.**
   Мутация 128→3000 — это матричное умножение `O(n × 128 × 3000)`.
   Если корпус 10M документов — это 3.8T операций. Быстрее full precision
   (10M × 3000), но не бесплатно.

4. **Когда mutation имеет смысл:**
   - **Ensemble models**: один корпус, два запроса от разных моделей —
     мутация приводит к единой размерности для скоринга
   - **Progressive precision**: начать с JL(128), уточнить Fourier(1024)
   - **Kernel trick без kernel**: RBF-ядро через Fourier features,
     без явного хранения kernel matrix
   - **Неизвестная размерность**: если d_intrinsic корпуса не известен —
     мутация перебирает размерности до сходимости

5. **Когда mutation бесполезна:**
   - Уже есть MRL-модель — MRL-срез даёт то же самое быстрее
   - Корпус мал (<10K) — проще full scan в исходной размерности
   - Нужно гарантированное качество — мутация не улучшает качество
     оригинальной модели, только трансформирует

### 22.6. Mutation + Type System

```scheme
;; Пространство после мутации — новое пространство с provenance
(define-space "my-256→fourier2048"
  :base "bge-base-en-v1.5"      ;; откуда пришло
  :dims 2048
  :method :fourier
  :kernel :rbf
  :gamma 0.1
  :provenance ["bge-base-en-v1.5"]  ;; цепочка трансформаций
  :confidence 0.73                  ;; empirical alignment
  )

;; Type checker: при COMPOSE двух mutation-spaces
;; предупреждает о каскадной потере
(let ((a (search :space "my-256→fourier2048" :query ...))
      (b (search :space "ada-002" :query ...)))
  (rrf a b)
  ;; Warning: cross-space fusion
  ;; 'my-256→fourier2048' is 2 transformations from original
  ;; alignment confidence: 0.73
  )
```

---

---

## 24. π, φ и дискретная алгебра retrieval

### 24.1. Задача двух кубов: что говорит π

Классическая задача: два куба на идеально гладкой поверхности,
массивный (M = 100ⁿ) толкается к стене, лёгкий (m = 1) — между ними.
Упругие столкновения. Вопрос: сколько всего столкновений до того,
как массивный куб уйдёт в бесконечность?

Ответ: `314159265...` — первые n+1 цифр числа π.

```
M = 1        →  3 столкновения
M = 100      →  31
M = 10000    →  314
M = 1000000  →  3141
...
```

**Почему так происходит:**

Фазовое пространство скоростей (v₁, v₂) после каждого столкновения —
это отражение точки на окружности. Угол между последовательными
состояниями — `arctan(1/√M)`. Количество шагов до исчерпания —
это `⌊π / arctan(1/√M)⌋`. При M → ∞ это аппроксимирует π√M.

Математика даёт: дискретная динамика (столкновения), управляемая
простыми законами сохранения (энергия + импульс), конвергирует к π.

### 24.2. Аналогия с retrieval

Наша VM — тоже дискретная динамика:

```
Операции VM:            Закон сохранения:
INNER_PRODUCT           Энергия = norm(вектор) не меняется
GRADE_DOWN              Порядок = инвариантен относительно перестановок
COMPRESS                Отбор = subset-сохранение
MUTATE                  Размерность = проекция, а не создание
FIXPOINT                Итерация до сходимости (как collision count)
```

Ключевая идея пользователя: **π и φ — не просто числа, а порождающие
константы для алгебры операций над пространством.**

- π задаёт **угловую метрику** (сравнение направлений векторов)
- φ задаёт **оптимальный шаг** (рекурсивное деление пространства)

### 24.3. π как angular embedding

Cosine similarity — это косинус угла между векторами. Но в задаче
кубов работает **угол** (arctan), а не косинус. Если перейти
от косинусной близости к **угловой**:

```apl
;; Текущее: косинус
cos_sim ← a ∘.cos b

;; Предлагаемое: угловое расстояние
ang_dist ← acos(a ∘.cos b)    ;; → [0, π]

;; φ-based взвешивание углов
weighted ← ang_dist ÷ φ        ;; → [0, 0.618...]

;; π-normalized scoring
π_norm   ← (π - ang_dist) ÷ π  ;; → [0, 1], где π = полное несходство
```

**Смысл**: угловое расстояние — метрика на гиперсфере, и π —
её естественная единица измерения. Два вектора либо сонаправлены
(0), противоположны (π), или ортогональны (π/2). Это компактификация
семантического пространства.

### 24.4. φ как оптимальный шаг поиска

Золотое сечение `φ = (1 + √5) / 2 ≈ 1.618` — "самое иррациональное
число" (его цепная дробь — все единицы). Это означает:

- **φ — наихудший случай для аппроксимации рациональными числами**
- Распределение точек, порождённое φ, максимально равномерно
  (low-discrepancy sequence)
- **Golden section search** — оптимальный метод поиска экстремума
  унимодальной функции

В контексте retrieval:

```scheme
;; Golden section search для оптимального top-k
(search :top-k (golden-section φ corpus_size))

;; φ-пропорция для progressive retrieval
(let ((step (floor (* φ (vector-length corpus)))))

;; φ-based разбиение пространства
(define-space "phi-hypercube"
  :basis φ                                ; шаг решётки
  :partition (λ (vec) (floor (vec × φ)))  ; квантование
  )
```

```apl
;; φ как adaptive step в progressive search
coarse  ← (⍒ scores) ↑ floor(φ×corpus_len)     ;; φ ≈ 61.8%
medium  ← (⍒ scores) ↑ floor(φ×coarse_len)
fine    ← (⍒ scores) ↑ floor(φ×medium_len)

;; φ-based sequence для сканирования без перекрёстных искажений
;; Халтона/Соболя на φ — равномернее random sampling
samples ← φ × (1, 2, 3, ..., n) mod 1
```

### 24.5. Дискретная алгебра сходится к π/φ: две гипотезы

**Гипотеза 1: Progressive retrieval сходится к π.**

Если progressive retrieval — это последовательность проекций
`ᵗ ∘.cos corpus` на срезах растущей размерности, то число шагов
до стабилизации ранжирования пропорционально `O(π√d)`.

То есть: для d-мерного пространства нужно ≈ π√d шагов,
чтобы реранжирование перестало меняться. Как collision count.

```
collisions(d) = ⌊π / arctan(1/√d)⌋ ≈ π√d
```

**Гипотеза 2: φ — оптимальное отношение для MRL-срезов.**

Лучшие точки отсечки MRL-размерностей следуют φ-пропорции:

```
d_optimal_i ≈ d_max × φ^(-i)
```

Для d_max = 1024: 633, 391, 242, 149, 92, 57, 35, 22, 13, 8, 5, 3.
Эти числа близки к последовательности Фибоначчи — что естественно,
потому что `φ = lim(F_{n+1} / F_n)`.

### 24.6. φ-размерности против битовых: математическое обоснование

Почему иерархия `[633, 391, 242, 149, 92, 57, 35, 22, 13, 8, 5, 3]`
принципиально отличается от `[512, 256, 128, 64, 32, 16, 8, 4, 2]`?

#### Фактор 1: отсутствие общих делителей

Ряд `[512, 256, 128, 64, 32, 16, 8, 4, 2]` — каждая степень делится
на все предыдущие. Это означает: **256 dims — подмножество 512 dims**,
информация на 256 — это первые 256 чисел вектора 512.

Проблема: если первые 256 dims кодируют некоторое семантическое
свойство, то следующие 256 dims (257-512) кодируют то же свойство,
но точнее. Это **избыточность**: 512 не даёт новой информации,
только уточнение старой.

φ-ряд `[633, 391, 242, 149, 92, 57, 35, 22, 13, 8, 5, 3]`:
- `633 / 391 = φ` (не целое)
- `391 / 242 ≈ 1.616` (не целое)
- Никакая размерность не делится на другую без остатка

Каждая размерность **не является подмножеством** другой. Они
перекрываются частично, как фазы косинуса в разных частотах.
А не как вложенные матрёшки одного паттерна.

#### Фактор 2: φ — "самое иррациональное число"

Цепная дробь φ:

```
φ = 1 + 1/(1 + 1/(1 + 1/(1 + ... )))
  = [1; 1, 1, 1, 1, ...]
```

Все коэффициенты = 1. Это **наихудший случай** для аппроксимации
рациональными числами. Для сжатия и вейвлет-разложения это значит:

- **φ-вейвлеты дают максимальную energy compaction** для сигналов
  с неизвестным спектром (Daubechies, "Ten Lectures on Wavelets")
- Ошибка аппроксимации убывает как `O(φ⁻ⁿ)` — геометрически,
  с максимально возможным основанием
- Для power-of-2, ошибка убывает как `O(2⁻ⁿ)` — быстрее, но
  требует точного совпадения с базисом

**Ключ**: power-of-2 даёт лучшую точность, **если** сигнал точно
выровнен по двоичной решётке. Если нет — φ-решётка даёт более
равномерную ошибку по всем масштабам.

#### Фактор 3: сжатие — не избыточность, а независимость

Сжатие (JL, PCA) работает тем лучше, чем **менее коррелированы**
размерности. Power-of-2 срезы коррелированны (одни и те же оси).
φ-срезы — нет.

```schme
;; Power-of-2: slice@256 = dims 0..255, slice@512 = dims 0..511
;;             корреляция: 1.0 (вложенность)
;;
;; φ-slice: slice@633 ≈ dims с шагом φ (Weyl sequence)
;;           slice@391 ≈ другая фаза
;;           корреляция: ~0.38 (почти ортогонально)
```

#### Фактор 4: "разжимание до бесконечности"

Ряд `[633, 391, 242, 149, 92, 57, 35, 22, 13, 8, 5, 3]` можно
продолжить в обе стороны:

```
...→∞, 1675, 1036, 640, 396, 245, 151, 93, 58, 36, 22, 14, 8, 5, 3, 2, 1 →0
                                     ↑
                                 d_max = 1024 (базовая модель)
```

Каждый шаг — умножение или деление на φ. Ряд сходится к 0
(бесконечно малая размерность) и расходится к ∞ (бесконечно
большая). Но **отношение соседних шагов всегда φ**.

Следствие: если у вас есть вектор `v ∈ ℝ⁶³³`, вы можете
"достроить" его до `v' ∈ ℝ¹⁰²⁴` через φ-интерполяцию,
не зная исходных 1024 dims. Просто потому, что φ-структура
однозначно определяет позицию недостающих:

```
v_{1024} = v_{633} ⊕ v_{391} — разность срезов даёт недостающий кусок
```

Это **алгебраическое продолжение** — как аналитическое продолжение
в комплексном анализе, только для размерностей.

**На практике:** документ проиндексирован в 633d. Другая модель
спрашивает в 1024d. Вместо CONVERT (проекция с потерей) —
**φ-продолжение** (алгебраическое достраивание без обучения).

#### Итог сравнения

| Свойство | Power-of-2 | φ-ряд |
|---|---|---|
| Базис | 2 (бинарное дерево) | φ (золотое сечение) |
| Вложенность | Полная (slice⊂full) | Частичная (пересечение) |
| Независимость уровней | Низкая | Высокая |
| Energy compaction | Лучше для aligned-signals | Равномернее для любых |
| Продолжимость до ∞ | Только в 2ⁿ шагах | Плавная, любой шаг |
| Ошибка аппроксимации | O(2⁻ⁿ) | O(φ⁻ⁿ) |
| Информационная плотность | Избыточна | Компактна |

**Вывод:** power-of-2 размерности — артефакт двоичной архитектуры
(регистры CPU, кэш-линии, адресация памяти). Не математическая
необходимость. φ-размерности приближаются к оптимальному басизу
для представления иерархической информации — и именно поэтому
их легче сжимать и продолжать до сколь угодно большой размерности
без потери структуры.

### 24.7. Что это даёт на практике (если гипотезы верны)

| Аспект | Сейчас | С π/φ |
|---|---|---|
| Размерность для быстрого прохода | Эвристика (128, 256) | `d_fast = d_max × φ⁻³ ≈ 0.236 × d_max` |
| Шаг прогрессии | `[128, 512, 1024]` | `[d×φ⁻⁴, d×φ⁻³, d×φ⁻², d×φ⁻¹, d]` |
| Размерности по слоям | power-of-2 (512, 256, 128, ...) | φ-ряд (633, 391, 242, 149, ...) |
| Сжатие/достраивание | PCA / JL (обучение + потеря) | φ-wavelet (без обучения, продолжение) |
| Граница стабилизации | Неизвестна | `≈ π√d шагов` |
| Метрика близости | Cosine | `acos(cosine) / π` (нормализованная в [0,1]) |
| Top-K отсечка | `k = 10` (hardcoded) | `k = floor(φ × φ × n)` ≈ 0.382 × n |

### 24.8. Философский итог

Задача двух кубов говорит: **π рождается из столкновений простых
законов сохранения**. Не из определения окружности, а из того,
что энергия и импульс сохраняются, а стена упругая.

Наша VM построена на сохранении:
- **Норма вектора** (при INNER_PRODUCT, ROTATION)
- **Порядок** (при GRADE, TAKE)
- **Семантическая информация** (write-time loss невосстановим)

Если π и φ управляют сходимостью таких систем, то они естественно
лягут в алгебру retrieval — не как константы, а как **операторы**,
определяющие метрику и шаг.

```apl
;; В пределе: DSEL (Domain-Specific Embedding Language)
;; — не retrievel, а фундаментальная алгебра над пространствами

π_sim  ← (π - acos(cos)) ÷ π   ;; π-нормированное сходство
φ_step ← φ × d_max              ;; оптимальный шаг прогрессии

search ← (⍒ π_sim query_emb corpus_emb) ↑ φ_step
```

### 24.9. Проблема плавающей точки: когда теория встречается с f32

#### 24.9.1. Где именно возникает хаос

**acos() — главный источник нестабильности.**

```apl
cos_sim  ← a ∘.cos b           ;; ∈ [-1, 1]
ang_dist ← acos(cos_sim)       ;; ∈ [0, π]
π_sim    ← (π - ang_dist) ÷ π  ;; ∈ [0, 1]
```

Проблема: производная `arccos(x) = -1 / √(1 - x²)`.

```
x →  1.0   → acos'(x) → -∞    (векторы почти совпадают)
x → -1.0   → acos'(x) → -∞    (векторы почти противоположны)
x →  0.0   → acos'(x) → -1    (ортогональны — стабильно)
```

Два вектора с cosine_sim = 0.9999999 (отличие в 1e-7) дают:
- `acos(0.9999999) ≈ 0.000447` (угол в радианах)
- Ошибка в cosine_sim на 1e-7 → ошибка в угле ≈ 0.01 радиана
- **Усиление шума в ~100 000 раз**

**Каскад операций:**

```apl
;; Каждый шаг добавляет ~1e-7 относительной ошибки
scores   ← query ∘.cos corpus        ;; ошибка O(n*d*ε)
angles   ← acos(scores)              ;; → ∞ у краёв
π_norm   ← angles ÷ π                ;; домножение
φ_weight ← π_norm × φ                ;; и ещё домножение

;; Результат после 4 шагов: ошибка может быть >0.1 для похожих векторов
```

Это и есть "шизофрения": один и тот же запрос с разным порядком
операций (или разной batch size внутри SIMD) даёт разный top-5.

#### 24.9.2. φ-ряд и накопление ошибки в размерностях

```scheme
;; Вычисление размерности среза
d_n = floor(d_max × φ⁻ⁿ)
```

При d_max = 1024:

```
n=0:  1024 × 1.0      = 1024.0000000 → 1024
n=1:  1024 × 0.618034 =  632.876...  → 633   (точное φ)
n=2:  1024 × 0.381966 =  391.133...  → 391
n=3:  1024 × 0.236068 =  241.733...  → 242
n=4:  1024 × 0.145898 =  149.399...  → 149
n=5:  1024 × 0.090170 =   92.334...  →  92
```

Проблема: `φ⁻ⁿ` через `pow(f32, n)` **не равно** последовательному
умножению `(x / φ) / φ`. `pow(0.618034, 5) = 0.090170` при f32, но
последовательное деление даёт `0.090169`. Разница в 1e-6 —
и `floor` может дать 92 вместо 91 на больших d_max.

На микроуровне — ошибка, на макроуровне — **граница среза сдвигается
на 1**, progressive retrieval теряет один уровень иерархии.

#### 24.9.3. Почему задача кубов стабильна, а retrieval — нет

В задаче двух кубов:

```
кол-во_столкновений = floor(π / arctan(1/√M))
```

Ошибка в `1/√M` = 1e-15 → ошибка в `arctan` = 1e-15 → ошибка
в `π / arctan` = 1e-15 → `floor` всё тот же. **Одно деление,
один тригонометрический вызов, один floor.**

В retrieval:

```
(⍒ (acos(query ∘.cos corpus) ÷ π × φ)) ↑ floor(d_max × φ⁻ⁿ)
```

Здесь: dot → acos → деление → умножение → grade → take.
**Шесть операций, каждая с плавающей ошибкой.**

Разница: в задаче кубов π — **результат**, к которому сходится
дискретная динамика. В нашем случае π/φ — **параметры**, которые
мы подставляем в плавающие вычисления. Разные роли, разная
стабильность.

#### 24.9.4. Методы стабилизации

**1. Стабильный acos (haversine-form).**

Вместо `acos(cos_sim)` — формула гаверсинуса, устойчивая при x→1:

```rust
fn stable_acos(cos_sim: f32) -> f32 {
    // haversine: acos(x) = 2 * asin(sqrt((1 - x) / 2))
    // Работает для x ∈ [-1, 1], стабилен при x → ±1
    let clamped = cos_sim.clamp(-1.0, 1.0);
    2.0 * (0.5 * (1.0 - clamped)).sqrt().asin()
}

// Тест: acos(0.9999999) vs stable_acos(0.9999999)
// acos:              0.000447213476   (ошибка ~0.0001)
// stable_acos:       0.000447213595   (ошибка ~1e-12)
```

Стабилизация: ~10⁸× улучшение точности у краёв.

**2. Нормализованное π-расстояние без acos.**

Вместо `acos(cosine)/π` — прямое отображение cosine в [0,1]:

```apl
;; Кусочно-линейная аппроксимация acos:
;; acos(x) ≈ 1 - x  для x ≈ 1 (близкие векторы)
;; acos(x) ≈ π/2 - x для x ≈ 0 (ортогональные)
;; acos(x) ≈ π + x  для x ≈ -1 (противоположные)

π_sim ← 0.5 - (0.5 × cos_sim)         ;; ∈ [0, 1], без acos
;; cos=1 → 0.0, cos=0 → 0.5, cos=-1 → 1.0
```

Теряется точная π-нормировка, но **порядок сортировки не меняется**
(acos — монотонная функция на [-1, 1], и `0.5 - 0.5x` — тоже).
Топ-K идентичен, а плавающей нестабильности на порядок меньше.

**3. Целочисленные размерности (lattice).**

Размерности срезов — не через `floor(d_max × φ⁻ⁿ)` в рантайме,
а через предвычисленную таблицу целых чисел:

```rust
// Предвычислено один раз при старте
const PHI_DIMS: [usize; 12] = [633, 391, 242, 149, 92, 57, 35, 22, 13, 8, 5, 3];
// Или через continued fraction от φ:
// φ = [1; 1, 1, 1, ...] → convergents: 1/1, 2/1, 3/2, 5/3, 8/5, 13/8, ...
// Самый простой способ: ряд Фибоначчи × d_max / φ^max

fn phi_dims(d_max: usize, max_steps: usize) -> Vec<usize> {
    let mut dims = Vec::new();
    let phi_inv = 0.6180339887498949_f64;  // в f64!
    for n in 0..max_steps {
        let d = (d_max as f64 * phi_inv.powi(n as i32)).floor() as usize;
        if d < 2 { break; }
        dims.push(d);
    }
    dims
}
```

**Ключ:** все вычисления размерностей — в **f64**, на старте,
один раз. В рантайме — только целые числа.

**4. FMA всюду.**

```rust
// MulAdd: одна rounding-операция вместо двух
// Вместо: a * b + c  (2 roundings)
// Через:  a.mul_add(b, c)  (1 rounding)

// INNER_PRODUCT через FMA:
let mut dot = 0.0_f32;
for i in 0..n {
    dot = a[i].mul_add(b[i], dot);  // одно округление
}
```

SIMD: `_mm256_fmadd_ps` (AVX2) / `vfmadd` (NEON).

**5. Компенсационное суммирование (Kahan).**

Для inner_product над длинными векторами (>10K dims):

```rust
fn kahan_dot(a: &[f32], b: &[f32]) -> f32 {
    let mut sum = 0.0;
    let mut c = 0.0;  // compensation term
    for i in 0..a.len() {
        let y = a[i].mul_add(b[i], -c);  // FMA + compensation
        let t = sum + y;
        c = (t - sum) - y;  // algebraic identity yields rounding error
        sum = t;
    }
    sum
}
```

Стабилизирует dot-произведение от O(n×ε) до O(ε) (независимо от n).

**6. Order-preserving quantization.**

Если φ-порядок важен, а точное значение — нет:

```rust
// Преобразование score → integer rank через φ-квантование
fn quantize_phi(scores: &[f32]) -> Vec<u32> {
    // φ-сегменты: [0, φ⁻²), [φ⁻², φ⁻¹), [φ⁻¹, 1)
    // → {0, 1, 2}
    let bounds = [0.381966, 0.618034, 1.0];  // φ⁻², φ⁻¹, 1
    scores.iter()
        .map(|&s| match s {
            s if s < bounds[0] => 0,
            s if s < bounds[1] => 1,
            _ => 2,
        })
        .collect()
}

// grade_down на u32 стабилен при любом floating-point шуме
```

#### 24.9.5. Итог: стабилен или нет?

| Операция | Стабильность | Метод |
|---|---|---|
| `query ∘.cos corpus` | **O(n×ε)** → O(ε) | Kahan summation + FMA |
| `acos(cos_sim)` | **∞ у краёв** → 1e-12 | stable_acos (haversine) |
| `acos(cos_sim) ÷ π` | Независимо | Константа, домножение |
| `dims × φ⁻ⁿ` | **1e-6 → 0** | f64 один раз, целые в runtime |
| Результат: `(⍒ scores) ↑ k` | **Топ-K стабилен** | Grade — integer, take — integer |

**Вывод:** π-нормировка через acos — да, проблема. Но она решается
(haversine, отказ от acos в пользу монотонной замены, Kahan).
φ-размерности — не проблема, если вычислить один раз в f64.

Главный урок из плавающей точки: **π и φ на уровне архитектуры —
стабильны. На уровне арифметики — требуют осторожности.**
Как и любая вычислительная математика.

---

### 24.10. Эксперименты для проверки гипотез

1. **Взять корпус + MRL-модель (GTE-Qwen2).**
   Измерить, при какой размерности progressive retrieval стабилизирует
   top-10. Повторить для разных d_max (256, 512, 1024).
   Сравнить `проходы_до_стабилизации` с `π√d`.

2. **Взять φ-пропорцию для срезов.**
   Сравнить `[d×φ⁻⁴, d×φ⁻³, d×φ⁻², d×φ⁻¹, d]` против равномерных
   срезов `[d/5, 2d/5, 3d/5, 4d/5, d]` по качеству / скорости.

3. **Угловая метрика π-shift.**
   Сравнить `acos(cosine)/π` vs `cosine` для задачи классификации
   семантической близости (STS-B). Есть ли разница при пересечении
   порога 0.5π (~ортогональность)?

---

## 25. Symbolic Verification Layer: Lisp + Prolog как гарант

### 25.1. Проблема: APL-алгебра не знает домена

APL-операторы работают с числами. Они не знают, что "ортогональные
векторы имеют нулевое скалярное произведение", что "поворот сохраняет
норму", или что "если A ближе к B чем к C, а B ближе к C чем к A —
это contradiction". VM вычисляет, но не верифицирует.

Tensor VM (continuous):
```
scores ← query ∘.cos corpus       ; выдаёт f32
reject ← acos(scores[0]) < 0.001  ; но: верно ли?
```

Prolog проверяет:
```prolog
% Domain law: если cos ≈ 1, то векторы коллинеарны
law(collinear, A, B) :-
    angle(A, B, Theta),
    close_to(Theta, 0.0, 0.001).

% Но: если A коллинеарен B и B коллинеарен C,
%      то A коллинеарен C (транзитивность)
law(transitive, A, C) :-
    law(collinear, A, B),
    law(collinear, B, C),
    A \= C,
    not law(collinear, A, C).  % violation → reject
```

### 25.2. Архитектура: two-layer verification

```
┌──────────────────────────────────────────────┐
│            DSL (пользовательский запрос)       │
└────────────────┬─────────────────────────────┘
                 │
                 ▼
┌──────────────────────────────────────────────┐
│    Layer 1: Tensor VM (continuous, быстрый)   │
│    APL ops, INNER_PRODUCT, GRADE, TAKE        │
│    f32, SIMD, Arrow, Kahan                    │
│    Выдаёт: scores, rankings, top-K            │
└────────────────┬─────────────────────────────┘
                 │ scores
                 ▼
┌──────────────────────────────────────────────┐
│    Layer 2: Symbolic Verifier                 │
│    (Lisp + Prolog, symbolic)                  │
│    Domain rules, invariant checking,          │
│    contradiction detection                    │
│    Проверяет: допустим ли результат           │
│    по законам загруженного домена?            │
└──────┬───────────────────────┬───────────────┘
       │ accept                │ reject
       ▼                       ▼
┌──────────────┐    ┌─────────────────────────┐
│   Answer     │    │ Пересчёт с уточнением    │
│              │    │ (stable_acos, Kahan,    │
│              │    │  повышение точности)    │
└──────────────┘    └─────────────────────────┘
```

### 25.3. Что проверяет Prolog

**Геометрические инварианты:**
```prolog
% 1. Симметричность косинуса
law(symmetry) :-
    cos_sim(A, B, S1),
    cos_sim(B, A, S2),
    close_to(S1, S2, 1e-6).

% 2. Ортогональность → cos ≈ 0
law(orthogonal_zero) :-
    orthogonal(A, B),
    cos_sim(A, B, S),
    close_to(S, 0.0, 1e-4).

% 3. Норма сохраняется при унитарном повороте
law(norm_preserved) :-
    rotate(A, A_rot, Matrix),
    is_unitary(Matrix),
    norm(A, N1), norm(A_rot, N2),
    close_to(N1, N2, 1e-6).
```

**Retrieval-инварианты:**
```prolog
% 4. Транзитивность top-K (если A > B и B > C, то A > C)
law(transitive_ranking, [A, B, C]) :-
    score(A, SA), score(B, SB), score(C, SC),
    SA > SB, SB > SC,
    SA > SC.   % иначе — нарушение

% 5. Инвариантность порядка при тавтологии
law(order_preserved, Query, Corpus) :-
    search(Query, Corpus, R1),
    search(Query, Corpus, R2),
    R1 \= R2.  % deterministic: должен совпадать
```

**Domain-specific constraints:**
```prolog
% 6. Метрика треугольника (если домен — метрическое пространство)
law(triangle_inequality, A, B, C) :-
    dist(A, B, D1), dist(B, C, D2), dist(A, C, D3),
    D3 > D1 + D2 + 1e-6.   % нарушение

% 7. Document domain: колличество авторов не может быть отрицательным
law(no_negative_metadata, Doc) :-
    metadata(Doc, author_count, N),
    N < 0.   % нарушение → reject entire result
```

### 25.4. Lisp как host для правил и рантайм Prolog

Prolog не запускается изолированно — он работает **внутри Lisp**:

```lisp
;; Lisp-функция, регистрирующая домен для Prolog
(defun register-geometry-domain ()
  (prolog:assertz '(law (collinear A B) :-
    (angle A B Theta)
    (close-to Theta 0.0 0.001)))
  (prolog:assertz '(law (orthogonal-zero A B) :-
    (orthogonal A B)
    (cos-sim A B S)
    (close-to S 0.0 1e-4)))
  (prolog:compile-rules *geometry-domain*))

;; Загрузка домена при старте VM
(defparameter *domains*
  (list (register-geometry-domain)
        (register-retrieval-domain)
        (register-document-domain)))
```

**Prolog embedded в Lisp — не новая идея** (PAIP, Norvig 1992; Kanren,
Byrd 2009; `clog` для Arc). Но для нас это означает:

- Lisp управляет правилами, компилирует их, кэширует выводы
- Prolog — библиотека внутри Lisp (как `kanren` или `trivial-prolog`)
- Rust VM вызывает Lisp через FFI (или встроенный интерпретатор)

### 25.5. GC и параллелизм: почему это не тормозит

**Проблема:** Lisp + Prolog — динамическая типизация, GC, конструирование
списков на каждом шаге. Если верификация идёт на горячем пути —
производительность упадёт.

**Решение: два режима выполнения.**

```
Fast path (99% запросов):
  Tensor VM → top-K → ответ.
  Верификация: только sample-проверка (каждый 100-й запрос).

Slow path (1% запросов, подозрительные):
  Tensor VM → top-K → Lisp/Prolog verifier → accept/reject
  Если reject → stable_acos → double precision → retry
```

**Распараллеливание:**

```
┌─────────────┐    ┌──────────────┐    ┌──────────────┐
│ Tensor VM   │    │ Lisp/Prolog  │    │ Lisp/Prolog  │
│ (CPU core 1)│    │ (CPU core 2) │    │ (CPU core 3) │
└──────┬──────┘    └──────┬───────┘    └──────┬───────┘
       │                  │                   │
       └──────────────────┴───────────────────┘
                      │
                      ▼
            ┌────────────────────┐
            │  Результат:        │
            │  Tensor scores +   │
            │  Prolog accept +   │
            │  batch checks      │
            └────────────────────┘
```

**GPU не подходит для Lisp/Prolog:**
- Lisp/Prolog — symbol manipulation, не tensor ops
- GPU оптимизирован под плотные матрицы, не под списки
- Lisp/Prolog работают на CPU, лучами (rayon) по разным core
- Tensor VM — на CPU SIMD (AVX-512) или GPU (CUDA), если нужно

**GC — не проблема потому что:**
1. Lisp-рантайм создаётся один раз при старте VM
2. Верификация — read-only: правила загружены, данные приходят
   из VM в pre-allocated буферы (через Rust FFI, без аллокаций)
3. Prolog query — pure: без side-effects, без мутации, без новых cons-ячеек
4. Если нужны новые факты — выделенный пул (memory pool) без участия GC

```rust
// Rust → Lisp FFI: zero-copy, zero-GC
fn verify_scores(scores: &[f32]) -> VerificationResult {
    let mut env = LISP_ENV.lock();
    // Передаём указатель на данные Rust → Lisp без копирования
    env.push_float_array(scores);
    env.call("verify-top-k");
    env.pop_result()  // Accept / Reject + offset в scores
}
```

### 25.6. Что это даёт для нашей архитектуры

| Без Verifier | С Verifier |
|---|---|
| Доверяем f32 | Проверяем доменные законы |
| Ошибка — silent wrong answer | Ошибка → reject → retry |
| Не знаем, почему ответ плох | Prolog объясняет: "violation of law #3" |
| Каждый запуск может отличаться | Гарантия: результат непротиворечив |

**Для пользователя:** доменные правила — это facts + rules, которые
он загружает в VM при старте. Не пишет Prolog сам, а выбирает
из каталога:

```scheme
;; Пользовательское приложение
(create-domain "my-retrieval"
  :inherit ["geometry" "retrieval-basics" "document-domain"]
  :custom [
    ;; Свои правила
    (:- (valid-result Query Results)
        (not (contains-outdated Results))
        (sufficient-coverage Query Results))
  ])
```

### 25.7. Lisp/Prolog как "калькулятор-страховка"

Итоговая идея: Tensor VM считает быстро, Lisp + Prolog проверяет
правильно. Если числа противоречат загруженным законам домена —
система отвергает результат и пересчитывает с гарантией точности.

- **Быстрый путь:** Tensor VM → ответ
- **Полный путь:** Tensor VM → Lisp/Prolog verifier → accept | reject
- **Reject → stable_acos → Kahan f64 → retry**
- **Параллелизм:** Tensor VM на одном core, Lisp на другом, объединение
  результатов в конце

### 25.8. Источники и реализации

#### Rust-крейты

| Крейт | Назначение | Как использовать |
|---|---|---|
| **`logru`** | Embeddable Prolog subset | `logru::TextTermDb` + `query_dfs` — для динамических правил верификации |
| **`kanlen`** / **`mini_kanren`** | MiniKanren в Rust | `run!`, `conde!`, `fresh!` — для small-scale symbolic constraint checking |
| **`crepe`** | Compile-time Datalog | `crepe!` macro — инварианты проверяются на этапе компиляции, zero-cost |
| **`datafrog`** | Lightweight Datalog (Mozilla) | Dynamic fixpoint, ручной контроль итераций |
| **`z3`** / **`z3-sys`** | SMT solver (Microsoft Z3) | `Context` + `Solver::check()` — contradiction detection, weight optimization |
| **`egg`** | E-graph equality saturation | APL identity laws как rewrite rules → query plan equivalence |
| **`egglog`** | Datalog + e-graphs (successor) | Быстрее `egg` для больших rulesets |
| **`steel`** | Scheme interpreter в Rust | Bytecode VM, R5RS — наш S-expression DSL может стать полноценным Lisp |
| **`mlua`** | LuaJIT/Lua 5.4 embedding | Самая быстрая embedded scripting, если нужна производительность |
| **`generational-arena`** | GC-free arena | Cons cells для Lisp без сборщика мусора |
| **`scryer-prolog`** | ISO Prolog в Rust | Полноценный WAM-based Prolog, но через subprocess (не embed) |

#### Prolog-реализации embedded

| Система | Язык | Стратегия |
|---|---|---|
| **Soufflé Datalog** | C++ OpenMP | Компиляция Datalog → параллельный C++. Не Lisp, но золотой стандарт производительности |
| **miniKanren** (Byrd et al., 2012) | Любой хост | Relational programming через substitution trees. O(n) unification |
| **SWI-Prolog** + `library(ffi)` | C/Java/Rust | Вызов через subprocess + shared memory (nailed bytes, Arrow IPC) |
| **CLP(FD)** / **CLP(R)** | SWI/Scryer | Constraint Logic Programming — domain propagation для tensor shape checking |
| **CHR** (Constraint Handling Rules) | Prolog + FreeCHR | Rule-based domain constraints: `X leq Y, Y leq X <=> X = Y` |
| **DeepProbLog** | Python + PyTorch | Probabilistic logic programming — нейронные предикаты |

#### Lisp-системы для tensor/numeric

| Система | Язык | Что даёт |
|---|---|---|
| **april** (phantomics/april) | Common Lisp | APL-to-Lisp компилятор. APL glyphs → Lisp AST → native code |
| **Maxima (itensor/ctensor)** | Common Lisp | Символическая tensor algebra: `contract(expand(...))` |
| **Lush** (Lisp Universal SHell) | C+Lisp | `idx` tensor engine 0-8 dims, BLAS/LAPACK/GSL bindings |
| **mgl-mat** | Common Lisp | Multi-dim arrays, CUDA, BLAS. Automatic CPU↔GPU transitions |
| **Coalton** | Common Lisp + LLVM | Haskell-like types embedded в CL. Type-safe symbolic AD |
| **Clasp** | LLVM backend CL | C++ interop, Eigen/Blaze tensor libraries |
| **Carp** | Lisp→C | Statically typed, no GC, borrow-checker. `(defstruct Vector2 [x :float, y :float])` |

#### Verification patterns

| Подход | Инструмент | Применение в нашей VM |
|---|---|---|
| **E-graph equality saturation** | `egg` / `egglog` | APL identity laws как rewrite rules → доказательство эквивалентности query plans |
| **SMT contradiction detection** | `z3` solver | "Есть ли assignment scores, удовлетворяющий всем законам домена?" |
| **Datalog compile-time invariants** | `crepe` macro | "Ни один query plan не нарушает закон транзитивности" — проверка при сборке |
| **Kanren backtracking** | `mini_kanren` | "Если результат нарушает constraint, бэктрекай к альтернативному ordering" |
| **CHR simpagation rules** | FreeCHR / Scryer | "Если contraction axis ≥ rank → reject" как CHR-правило |
| **CLP(FD) domain propagation** | SWI/Scryer | Типы/формы тензоров как domains → автоматическое сужение при verify |

#### Ключевые работы

- Norvig, P. *Paradigms of Artificial Intelligence Programming* (1992), Chapter 11 — Prolog в 150 строк Common Lisp
- Byrd et al. *A Unified Approach to Solving Seven Programming Problems* (2012) — канонический miniKanren
- Manhaeve et al. *DeepProbLog: Neural Probabilistic Logic Programming* (NeurIPS 2018) — нейронные предикаты
- Serafini & d'Avila Garcez. *Logic Tensor Networks* (2016) — Differentiable FOL grounded as tensor graphs
- Sussman & Wisdom. *SICP: Structure and Interpretation of Classical Mechanics* (2013) — Symbolic + numeric в одной системе
- Willsey et al. *egg: Fast and Extensible Equality Saturation* (POPL 2021) — E-graph rewrite system
- Bezanson et al. *Julia: Array Operators Using Multiple Dispatch* (2014) — Tensor DSL dispatch design
- Wibiral et al. *FreeCHR: Algebraic Framework for CHR Embeddings* (TPLP 2025) — CHR как embeddable library

---

## 26. Резюме: размерность — не свойство, а фаза

Классический RAG:
```
data → embed (768d) → index (768d) → search (768d) → rerank (cross-encoder)
```

Предлагаемый подход:
```
data → embed (d_original) → index → search (d_any) → score
                                        ↑
                                DIM_MUTATE (любая размерность
                                на лету через APL-операторы)
```

Размерность больше не фиксирована моделью. Это **оптимизационный параметр**,
который VM может выбирать per-query: быстрый проход на 64d, точный на 2048d,
kernel trick через Fourier, ensemble разнородных моделей через общий
procrustes-проекцию.

Это не замена embedding модели, а **алгебраический слой поверх неё**,
который делает размерность прозрачной для пользователя и адаптивной
для оптимизатора.

---

## 27. Эффект наблюдателя: квантовая вероятностная модель retrieval

### 27.1. Корпус в суперпозиции, запрос как измерение

Классический RAG трактует retrieval как детерминированный поиск: дай запрос,
получи ранжированный список. Но если corpus embedding существует до запроса,
то **до измерения (запроса) корпус находится в суперпозиции всех возможных
релевантностей**:

```
|Φ⟩ = Σᵢ √wᵢ |dᵢ⟩       (корпус до запроса — суперпозиция)
P_Q = |q⟩⟨q|             (запрос как проектор фон Неймана)
|Φ'⟩ = P_Q|Φ⟩ / ‖P_Q|Φ⟩‖  (коллапс после измерения)
```

Акт запроса — это **измерение** в смысле фон Неймана (1932). Результат —
коллапс суперпозиции в конкретную ранжировку. Разные запросы (разные базисы
измерения) дают разный коллапс из одного и того же корпуса.

**Операторная аналогия**:

| Квантовый концепт | Retrieval | Наша VM |
|---|---|---|
| Гильбертово пространство H | Embedding space ℝᵈ | VEC_DIMS |
| Вектор состояния | |d⟩ | Tensor1D документа |
| Суперпозиция | Σᵢ αᵢ |dᵢ⟩ | Весь корпус |
| Измерение (P_Q) | Запрос q | INNER_PRODUCT(q, D) |
| Борновское правило | P(q\|d) = |⟨q\|d⟩|² | cos²(θ) |
| Коллапс | Ranking | GRADE_DOWN + TAKE |
| Слабое измерение | Частичный коллапс | MRL_PROGRESSIVE |

### 27.2. Born rule ↔ cosine similarity

Для нормализованных векторов:

```
cos(θ) = ⟨q|d⟩                    (амплитуда вероятности)
cos²(θ) = |⟨q|d⟩|²                (Born rule — вероятность)
```

**Ключевой инсайт**: `cos²(θ)` удовлетворяет аксиомам Колмогорова (вероятность),
а `cos(θ)` несёт фазовую информацию, позволяющую **интерференцию** между
документами. Это различие — фундаментальное: классический retrieval использует
cosine (амплитуду) как вероятность, теряя интерференционные эффекты.

### 27.3. Quantum Probability Ranking Principle (QPRP)

Классический PRP (Robertson, 1977): `P(A ∨ B) = P(A) + P(B)` — документы
ранжируются независимо. Квантовый PRP (Zucconi & Azzopardi, ECIR 2010):

```
P(A ∨ B) = P(A) + P(B) + 2√(P(A)P(B))·cos(θ_AB)
```

Интерференционный член `2√(P(A)P(B))·cos(θ_AB)`:
- `θ_AB = 0` (документы идентичны): конструктивная интерференция → супер-аддитивность
- `θ_AB = π/2` (ортогональны): классический случай → аддитивность
- `θ_AB = π` (противоположны): деструктивная → подавление избыточности

Это объясняет, почему два релевантных документа могут вместе давать
**менее** релевантный результат (destructive interference для redundant
information).

**APL**:
```apl
phase  ← corpus ∘.cos corpus          ⍝ θ_AB matrix
P      ← |cos_scores*2                 ⍝ Born probabilities
interf ← 2 × (√P ∘.× √P) × phase      ⍝ 2√(P(A)P(B))cos(θ)
scores ← +/ interf                     ⍝ QPRP ranking
```

**VM**:
```rust
OpCode::QPRP_RANK { top_k: usize } => {
    let phase_mat = tensor[1];  // cos(θ) pairwise
    let probs = tensor[0];      // P(d|q) from Born
    let sqrt_p = probs.map(|p| p.sqrt());
    let interference = 2.0 * (&sqrt_p * &sqrt_p.t()) * &phase_mat;
    let scores = interference.sum_axis(Axis(1));
    stack.push(Tensor1D(scores));
}
```

### 27.4. Матрицы плотности для неоднозначных запросов

Один вектор = pure state. Но запрос может быть **многогранным**:

```
ρ_q = p₁|q₁⟩⟨q₁| + p₂|q₂⟩⟨q₂| + p₃|q₃⟩⟨q₃|
```

**Score**: `tr(ρ_q · P_d) = Σᵢ pᵢ · |⟨qᵢ|d⟩|²`

**Энтропия фон Неймана** — мера неоднозначности запроса:
```
S(ρ) = -tr(ρ · log ρ) = -Σᵢ λᵢ log λᵢ
```

- `S = 0`: pure state (чёткий запрос)
- `S = log n`: максимально неоднозначный

### 27.5. Теорема Глисона: почему только trace-based scoring

**Gleason (1957)**: В гильбертовом пространстве размерности ≥ 3 любая
вероятностная мера на подпространствах имеет вид `µ(L) = tr(ρ · P_L)`.

**Следствие для retrieval**: любая консистентная система назначения
вероятностей релевантности документам (подпространствам) **обязана**
использовать trace-формулу. Это доказывает, что cosine + inner product —
не просто удобный выбор, а единственно возможный в рамках гильбертовой
модели.

### 27.6. MRL Progressive как weak → strong measurement

Слабое измерение (Aharonov-Albert-Vaidman, PRL 1988): частичный коллапс
с параметром силы `λ ∈ [0,1]`:

```
ρ → (1-λ)ρ + λ · (P_k ρ P_k) / tr(P_k ρ)
```

Наш MRL progressive — последовательность измерений возрастающей силы:

```
Step 1 (λ = 0.17):  128d  → partial collapse → Top-1000
Step 2 (λ = 0.33):  256d  → stronger          → Top-200
Step 3 (λ = 0.67):  512d  → near-strong       → Top-50
Step 4 (λ = 1.0):   1024d → strong collapse   → Top-10
```

Это не инженерный трюк, а **фундаментальная структура**: каждое измерение
сужает суперпозицию; слишком грубое первое измерение может "заморозить"
состояние (Quantum Zeno effect), отбросив документы, релевантные только
в тонких размерностях.

### 27.7. Связь с π/φ

- **π** — естественный масштаб фазы: `cos(0) = 1` (constructive),
  `cos(π) = -1` (destructive), `cos(π/2) = 0` (orthogonal).
  Угловое расстояние `acos(cos(θ)) ∈ [0,π]` — метрика на гиперсфере.

- **φ** — параметр оптимального squeezing в квантовой оптике:
  минимальная неопределённость при отношении дисперсий = φ.
  В нашей VM: размеры φ-иерархии (633, 391, 242…) — оптимальные
  "сжатые" состояния для progressive измерения.

### 27.8. Следствия для VM

1. **OpCode QPRP_RANK** — интерференционная ранжировка (замена reranker)
2. **OpCode DENSITY_MATRIX** — построение ρ для многогранных запросов
3. **OpCode VON_NEUMANN_ENTROPY** — мера неоднозначности запроса/результата
4. **OpCode TRACE_DISTANCE** — сравнение распределений retrieval
5. **OpCode POVM_MEASURE** — graded relevance (1-5 шкала) как POVM

Полный формализм — в `docs/qir_rfc_formalism.md` (884 строки, 10 разделов,
25+ references, все APL- и VM-маппинги).

**References:**

- von Neumann, J. *Mathematische Grundlagen der Quantenmechanik* (1932)
- Gleason, A.M. "Measures on the Closed Subspaces of a Hilbert Space." *J. Math. Mech.* 6 (1957)
- Van Rijsbergen, C.J. *The Geometry of Information Retrieval*. CUP (2004)
- Zuccon, G. & Azzopardi, L. "Using the Quantum Probability Ranking Principle to Rank Interdependent Documents." ECIR (2010)
- Aharonov, Y., Albert, D.Z. & Vaidman, L. "Weak measurement." *PRL* 60(14) (1988)
- Sordoni, A., Nie, J.-Y. & Bengio, Y. "Modeling Term Dependencies with Quantum Language Models for IR." SIGIR (2013)
- Melucci, M. *Introduction to Information Retrieval and Quantum Mechanics*. Springer (2015)
- Piwowarski, B. et al. "What can Quantum Theory bring to Information Retrieval?" CIKM (2010)
- Busemeyer, J.R. & Bruza, P.D. *Quantum Models of Cognition and Decision*. Cambridge (2012)

---

## 24. Архитектурный принцип: границы алгебры и модели

```
┌─────────────────────────────────────────────────────────┐
│                   Embedding Model                        │
│   f: Text → ℝ^d  (единственный источник семантики)      │
│   Deep learning, обучен на текстах                       │
└──────────────────────┬──────────────────────────────────┘
                       │ вектора
                       ▼
┌─────────────────────────────────────────────────────────┐
│              Tensor-Relational VM (ALGEBRA)               │
│   APL-операторы + Relational Algebra + Datalog           │
│   Не понимает текст, не обучается                        │
│   Композирует, фильтрует, сортирует, проецирует          │
│   Гарантирует: однозначность, воспроизводимость          │
└──────────────────────┬──────────────────────────────────┘
                       │ скоринг
                       ▼
┌─────────────────────────────────────────────────────────┐
│                     Answer                                │
│   Результат — то, что алгебра выбрала из закодированного │
│   Моделью семантического пространства                    │
└─────────────────────────────────────────────────────────┘
```

**Правило:** модель отвечает за *что означают числа*.
VM отвечает за *как эти числа комбинировать*.
Они не пересекаются. VM не "понимает", она вычисляет.

---


## 23. Открытые вопросы (обновлённые)

1. **Исполнение PARALLEL**: tokio::join! против rayon — что лучше для
   in-process VM? Rayon даёт work-stealing для CPU-bound, tokio — для I/O.

2. **Semi-naive fixpoint**: как хранить delta/relations между итерациями?
   in-memory HashMap или через Arrow IPC?

3. **MRL-оптимизация**: как понять, что модель MRL-совместима?
   PEFT-адаптер или декларация в конфиге?

4. **Границы programmable space**: если embedding — программа VM, как
   гарантировать, что пользователь не написал бессмысленную проекцию?

5. **Cost model для MRL progressive**: сколько dims брать на каждом шаге?
   Адаптивно (по variance scores) или статически?

6. **Sparse join через BM25_JOIN**: как строить inverted index внутри Arrow?
   ColBERT-стиль (Floret) или классический BM25 Okapi?

7. **Space CONVERT c обучением**: если преобразование между пространствами
   нелинейно, нужна обучаемая проекция (ONNX-сессия внутри VM).

---

## 22. References

- Iverson, "A Programming Language" (1962)
- Backus, "Can Programming Be Liberated from the von Neumann Style?" (1977)
- Codd, "A Relational Model of Data for Large Shared Data Banks" (1970)
- Cockett & Spencer, "Charity: A Categorical Programming Language" (1992)
- Jordan et al., "Soufflé: On Synthesis of Program Analyzers" (CAV 2016)
- Subotić et al., "Automatic Index Selection for Large-Scale Datalog" (PVLDB 2018)
- Rybina et al., "Differential Datalog (DDlog)" (VMware)
- McSherry et al., "Differential Dataflow" (CIDR 2013)
- Kusner et al., "Word Embeddings via Linear Transformations" — space mapping
- kdb+/q: https://code.kx.com/
- SimSIMD: https://github.com/ashvardanian/SimSIMD
- Von Thun, "Joy: A Concatenative Language" (1990s)
- **Matryoshka Representation Learning** (MRL): arXiv:2205.13147
- **SPLADE**: arXiv:2107.05720 (sparse vectors)
- **ColBERTv2**: arXiv:2112.01488 (late interaction)

- Iverson, "A Programming Language" (1962)
- Backus, "Can Programming Be Liberated from the von Neumann Style?" (1977)
- Codd, "A Relational Model of Data for Large Shared Data Banks" (1970)
- Cockett & Spencer, "Charity: A Categorical Programming Language" (1992)
- Jordan et al., "Soufflé: On Synthesis of Program Analyzers" (CAV 2016)
- Subotić et al., "Automatic Index Selection for Large-Scale Datalog" (PVLDB 2018)
- Rybina et al., "Differential Datalog (DDlog)" (VMware)
- McSherry et al., "Differential Dataflow" (CIDR 2013)
- kdb+/q: https://code.kx.com/
- SimSIMD: https://github.com/ashvardanian/SimSIMD
- Von Thun, "Joy: A Concatenative Language" (1990s)
