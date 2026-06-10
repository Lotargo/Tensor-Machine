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

## 15. Открытые вопросы

1. **Исполнение PARALLEL**: tokio::join! против rayon — что лучше для
   in-process VM? Rayon даёт work-stealing для CPU-bound, tokio — для I/O.

2. **Semi-naive fixpoint**: как хранить delta/relations между итерациями?
   in-memory HashMap или через Arrow IPC?

3. **Datalog типизация**: нужна ли типизация переменных (symbol, float, vector)?
   Soufflé-style декларация `.decl` или type inference?

4. **Cost model для similarity join**: селективность для embedding-пространств
   оценивается через sampling?

5. **Границы COMPOSE**: если COMPOSE вложен, как VM управляет стеком между
   sub-programs? Сохранять/восстанавливать высоту стека?

---

## 16. References

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
