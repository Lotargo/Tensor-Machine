# DSL Research Synthesis: Towards a Unified Tensor Query Language

> Исследование альтернативных и забытых подходов к DSL для векторного RAG.
> June 2026

---

## 1. Мотивация

Текущий DSL (S-выражения на `lexpr`) справляется со своей задачей, но архитектурно
он остаётся "просто парсером с match на символах". Вопрос: можно ли пересоздать
DSL с нуля, собрав самые сильные идеи из разных парадигм — включая те, что были
забыты или не реализованы из-за сложности?

---

## 2. Исследованные направления

### 2.1 APL / J / K — тензорные операторы

**Источник:** Iverson (1962), "A Programming Language"; Turing Award 1979.

**Суть:** Ранг-полиморфная нотация для операций над массивами. APL-оператор
работает на скаляре, векторе, матрице — без broadcasting boilerplate.

**Ключевые операторы для RAG:**

| Операция | APL | Описание |
|---|---|---|
| Inner product (dot) | `a +.× b` | Сумма произведений = cosine similarity |
| Inner product (max-min) | `a ⌈.⌊ b` | Max-min similarity (нечёткая логика) |
| Outer product | `a ∘.F b` | Матрица всех попарных F |
| Grade down | `⍒ v` | Индексы сортировки по убыванию |
| Grade up | `⍋ v` | Индексы сортировки по возрастанию |
| Double grade | `⍋⍋ v` | Ранг каждого элемента |
| Take | `k ↑ v` | Первые K элементов |
| Compress | `mask / v` | Выбор по булевой маске |
| Where | `⍸ mask` | Индексы где mask=1 |
| Interval index | `bins ⍸ x` | Биннинг/пороги |
| Reduce | `F/ v` | Свёртка (сумма, макс, и т.д.) |
| Scan | `F\ v` | Префиксная свёртка |
| Reshape | `dims ⍴ v` | Изменение формы тензора |
| Transpose | `⍉ m` | Транспонирование |
| Membership | `a ∊ b` | Поиск в коллекции |

**Почему не прижилось:** Специальные символы (⍋⍒⌈⌊⍉) создали барьер входа.
"Write-only language" репутация. Но идеи на десятилетия опередили время.

**Почему актуально для RAG:**
- Embedding — это вектора. Весь RAG pipeline — операции над векторами и матрицами.
- APL'ный "think in arrays, not loops" — это именно то, что нужно для тензорного ядра.
- kdb+/q (KX) доказала: APL-подход работает в production для time-series данных,
  а KDB.AI добавил поверх векторный поиск (HNSW, IVF, DTW).

**Отличия J от APL:**
- ASCII вместо спецсимволов: `/:` = grade up, `\:` = grade down
- Rank operator `"` — универсальный контроль размерности
- Tacit programming (hooks/forks) — point-free композиция

**Отличия K/q от APL:**
- Минимализм: ~20 примитивов
- No rank operator — только `'` (each) и `+` (flip)
- Нативная поддержка time-series: nanosecond timestamps, as-of joins
- kdb+ — колоночная in-memory БД с разбивкой по времени

---

### 2.2 Datalog — рекурсия для retrieval

**Источник:** Codd (1970) + "Datalog" name from 1980s; Soufflé Datalog (2017-).

**Суть:** Дедуктивная БД на Horn-клаузах с рекурсией и фиксированной точкой.
В отличие от Prolog — bottom-up evaluation, гарантированная завершимость (P-time).

**Multi-hop retrieval как Datalog:**

```datalog
// EDB
cites(doc1, doc2).
contains_entity(doc1, "Einstein").
relates("Einstein", "relativity", 0.85).

// Recursive path
hop(doc, doc, 0) :- seed_doc(doc).
hop(d1, d3, N+1) :- hop(d1, d2, N), cites(d2, d3).

// Вывод
relevant_entity(E) :- seed_doc(D), contains_entity(D, E).
relevant_entity(E) :- hop(_, D, _), contains_entity(D, E).
```

**Стратегии с fallback (Prolog-style):**

```prolog
% Если dense retrieval уверен — используем его
answer(Q, A) :- dense_retrieve(Q, A), confidence(A, C), C >= 0.7.
% Иначе — разбиваем и идём multi-hop
answer(Q, A) :- not answer(Q, _), decompose(Q, [Q1, Q2]),
                answer(Q1, A1), answer(Q2, A2), merge(A1, A2, A).
```

**Soufflé Datalog:**
- Компиляция: `.dl` → RAM IR → параллельный C++ → native binary
- Magic-set rewriting — оптимизация top-down запросов для bottom-up execution
- Специализированные B-tree и Brie (lock-free trie) для high-concurrency
- Eqrel — equivalence relation через union-find (линейное время вместо квадратичного)

**Отличия Flora-2 / F-logic:**
- Object-oriented Datalog: `alice[parent -> bob]`.
- Defeasible reasoning: strict vs defeasible rules (естественные fallback стратегии).

**Применимость:**
- Multi-hop retrieval как рекурсивный запрос — идеальное совпадение
- Правила заменяют ручной if/else orchestration
- Soufflé показывает, что Datalog может быть быстрым (компиляция в C++)

---

### 2.3 Backus' FP (1977) — алгебра программ

**Источник:** John Backus, "Can Programming Be Liberated from the von Neumann Style?"
(Turing Award Lecture, 1977).

**Суть:** Point-free функциональная система с комбинирующими формами (combining forms).
Никаких переменных, никаких присваиваний — только композиция чистых функций.

**Combining forms:**

| Форма | Обозначение | Смысл |
|---|---|---|
| Composition | `f ∘ g` | Применить g, потом f |
| Construction | `[f₁, ..., fₙ]` | Применить все, собрать результаты |
| Condition | `p → f; g` | Если p то f иначе g |
| Insert | `/f` | Reduce right-to-left |
| Apply to all | `α f` | Map над коллекцией |
| Constant | `¯ x` | Функция, всегда возвращающая x |

**Ключевое открытие Бэкуса: алгебра программ.**

Можно доказывать эквивалентность преобразований:

```
f ∘ [g, h]  =  [f ∘ g, f ∘ h]     (distribution)
/⊕ ∘ αg     =  /(⊕ ∘ (g × g))     (promotion)
p → /⊕ ∘ α(g); h ∘ /⊕ ∘ α(g)      (fusion)
```

**Применимость к RAG:**

```fp
// Пайплайн как композиция
rerank ∘ filter(score>0.7) ∘ embed ∘ query

// Параллельные стратегии
[vector_search, bm25_search, graph_traverse]

// Conditional: если confidence низкий — multi-hop
low_confidence? → decompose ∘ multi_hop; direct_answer
```

Каждая комбинация — это закон переписывания плана запроса. Можно доказать,
что два плана эквивалентны, и выбрать более быстрый.

**Почему не прижилось:**
- Требует мышления "в стиле combinators" (как APL — высокий порог)
- Победил лямбда-исчисление (ML, Haskell, Lisp)
- Но алгебра живёт в Bird-Meertens formalism, Haskell's list fusion

---

### 2.4 Codd's Relational Algebra — формальные основания

**Источник:** E.F. Codd, "A Relational Model of Data for Large Shared Data Banks" (1970).

**Суть:** Алгебраическая система над отношениями (множества кортежей).
Пять примитивных операторов с гарантией замкнутости (closure).

**Классические операторы:**

| Оператор | Обозн. | Смысл |
|---|---|---|
| Selection | `σcond(R)` | Фильтр по условию |
| Projection | `πcols(R)` | Выбор колонок |
| Cartesian product | `R × S` | Все пары кортежей |
| Union | `R ∪ S` | Объединение |
| Difference | `R ∖ S` | Разность |

Производные: Join `R ⋈ S` (θ-join, natural, outer), Rename `ρ`, Division `÷`,
Aggregation `γ`.

**Tensor-Relational Algebra (адаптация для RAG):**

| Оператор | Смысл |
|---|---|
| `σdist < 0.8` (R) | Фильтр по косинусному расстоянию |
| `R ⋈sim Q` | Join по семантической близости (nearest neighbor) |
| `ρscore` | Упорядочивание по скору |
| `R ∪ S` | Объединение результатов (merge) |
| `γpool (R)` | Pooling / attention над результатами |

**Замкнутость — ключевое свойство:**
Каждый оператор возвращает tensor-relation. Это позволяет строить вложенные
выражения и оптимизировать их через rewrite rules (как в классических БД).

**Применимость:**
- Переписывание планов: `σc(σd(R)) = σc∧d(R)`
- Fusion: `π(σ(R))` может выполняться за один проход
- Join ordering: `(R ⋈ S) ⋈ T` vs `R ⋈ (S ⋈ T)` — формально различны,
  можно выбирать оптимальный

---

### 2.5 Concatenative Languages — стековый pipeline

**Источник:** Forth (Moore, 1970), Joy (von Thun, 1990s), Factor (2003+).

**Суть:** Постфиксная запись + стек + quotations (код как данные).
Композиция — просто конкатенация программ.

**Базовые combinators:**

| Combinator | Эффект | Смысл |
|---|---|---|
| `dip` | `(x [q] → x)` | Сохранить x, применить q, восстановить |
| `keep` | `(x [q] → x)` | Применить q к x, оставить x |
| `bi` | `(x [p] [q] → p(x) q(x))` | Две функции к одному x |
| `tri` | `(x [p] [q] [r] → p(x) q(x) r(x))` | Три функции |
| `bi@` | `(x y [q] → q(x) q(y))` | Одна функция к двум аргументам |
| `spread` | `(x y [q] → q(x, y))` | Функция от кортежа |

**Пример retrieval pipeline:**

```
query "deep learning"
  dup [ dense_search 20 ] [ bm25_search 20 ] bi   // ветвление
  rrf                                                // Reciprocal Rank Fusion
  (score>0.7)/                                       // фильтр
  take 3                                             // top-3
  llm_answer
```

**Joy's category-theoretic insight:**
Конкатенативный язык — это внутренний язык замкнутой моноидальной категории.
- Объекты = stack types (A — B)
- Морфизмы = программы (stack transformations)
- Композиция = конкатенация
- Identity = пустая программа

Quotation `[f]` — это интернализация морфизма как данных.
Combinator — это естественное преобразование.

**Почему не прижилось:**
- Постфикс обратен естественному языку
- Debugging требует ментальной симуляции стека
- Factor's type system сложна и непонятна
- Нет индустриального мейнстрима

---

### 2.6 Charity / Categorical Programming

**Источник:** Cockett & Spencer (1992); "Charity: A Categorical Programming Language".

**Суть:** Чисто функциональный язык без общей рекурсии — только catamorphisms
(folds) и anamorphisms (unfolds). Гарантированная завершимость.

**Для RAG критически важный паттерн — Hylomorphism:**

```
answer = cata(ℱ) ∘ ana(𝒢)(query)
       = fold(pooling) ∘ unfold(retrieval)
```

Прямое отображение на retrieve-then-read:

```
retrieve: Query → Candidates   (anamorphism — разворачиваем запрос в набор)
synthesize: Candidates → Answer (catamorphism — сворачиваем в ответ)
```

**Применимость:**
- Attention pooling — это catamorphism над последовательностью
- Multi-hop — hylomorphism: unfold путей, fold в ответ
- Формальная корректность: если attention — истинный catamorphism,
  он гарантированно хорошо себя ведёт при композиции

**Почему не прижилось:** Не-Turing-complete языки непрактичны. Но паттерны
(fold/unfold/hylomorphism) дают принципиальный словарь для RAG.

---

### 2.7 SETL — set-theoretic retrieval

**Источник:** Schwartz (1969), "SETL: A Set-Theoretic Language".

**Суть:** Всё есть множество. Set comprehensions, powerset, quantifiers (∃, ∀).

**Для RAG:**

```
// Результат — множество документов
{d ∈ corpus | similarity(query, embed(d)) > 0.9 ∧ d.author ∈ trusted}

// Объединение стратегий
vector_results ∪ keyword_results

// Квантор существования: есть ли документ, удовлетворяющий условию?
∃d ∈ corpus (contains(d, "Einstein") ∧ cites(d, "relativity"))
```

Map как set-of-pairs: `E: Documents → Vectors` — embedding как отношение.
Query — обратное отображение.

**Применимость:**
- Set comprehension — естественная нотация для retrieval
- Quantifier-based reasoning: "существует ли подтверждение?" / "все ли источники согласны?"
- Powerset для рекурсивного разбиения запросов

---

## 3. Синтез: Предлагаемая архитектура

### Трёхуровневая модель

```
┌─────────────────────────────────────────────┐
│  Layer 3: Surface Syntax                     │
│  (Concatenative + S-expressions)             │
│  Композиция пайплайнов, ветвление, quoting  │
├─────────────────────────────────────────────┤
│  Layer 2: Query Algebra (IR)                 │
│  (Relational Algebra + Datalog)              │
│  Формальная семантика, rewrite rules,        │
│  рекурсия, стратегии с правилами             │
├─────────────────────────────────────────────┤
│  Layer 1: Tensor Operators                   │
│  (APL/J operators над Arrow arrays)          │
│  Rank-polymorphic ops, broadcasting,         │
│  inner/outer product, reduction              │
├─────────────────────────────────────────────┤
│  Layer 0: Tensor VM (execution)              │
│  (уже существующая)                          │
└─────────────────────────────────────────────┘
```

### Layer 1: APL-inspired Tensor Operators

APL-операторы, отображённые на Rust и Arrow arrays:

```rust
// Текущее состояние (условно):
fn cosine_similarity(query: &[f32], corpus: &[f32]) -> Vec<f32> {
    // цикл с dot product
}

// Предложение: rank-polymorphic operators
trait TensorOp {
    type Elem;
    fn inner_product<F, G>(&self, other: &Self, f: F, g: G) -> Self;
    fn outer_product<F>(&self, other: &Self, f: F) -> Array2D<Self::Elem>;
    fn reduce<F>(&self, axis: usize, f: F) -> Self;
    fn grade_down(&self) -> Vec<usize>;
    fn compress(&self, mask: &[bool]) -> Self;
    fn take(&self, k: usize) -> Self;
}
```

### Layer 2: Query Algebra (Relational + Datalog)

```rust
enum QueryOp {
    // Relational
    Select(Filter),
    Join(JoinKind, SimilarityMetric, f64),
    Union,
    Difference,
    
    // APL-tensor
    GradeDown,
    Take(usize),
    Compress(Filter),
    OuterProduct(SimilarityMetric),
    
    // Datalog-recursive
    Fixpoint(Box<QueryOp>, Box<QueryOp>),
    Rule(Vec<Rule>),
    
    // FP-combinators
    Compose(Vec<QueryOp>),
    Parallel(Vec<QueryOp>),
    Condition(Box<QueryOp>, Box<QueryOp>, Box<QueryOp>),
}
```

### Layer 3: Surface Syntax (гибрид)

Вариант A — S-выражения (эволюция текущего):

```scheme
;; APL-style внутри S-expressions
(pipeline
  :steps [
    [(hybrid :query "deep learning" :top-k 20)    ; текущий DSL
     (bm25   :query "deep learning" :top-k 20)]
    :operator rrf                                   ; fusion
    (compress :field score :op >= :value 0.7)
    (take 3)
    (llm-generate)
  ])

;; Datalog-правила
(rules
  (answer Q A :- (dense Q A) (confidence A C) (>= C 0.7))
  (answer Q A :- (not (answer Q _)) (decompose Q Qs)
                 (map answer Qs As) (merge As A)))

;; APL-notasyon (если захочется)
(+.× query_embed corpus_embeddings)    ;; inner product
(⍒ scores)                              ;; grade down
(↑ 5 (⍒ scores))                        ;; top 5 indices
(compress (> scores 0.7) results)       ;; filter
```

Вариант B — Concatenative (стековый):

```
query "deep learning"
  [ dense 20 ]
  [ bm25  20 ]
  bi        # parallel → stack: (dense_result bm25_result)
  rrf       # Reciprocal Rank Fusion
  [ (score > 0.7) compress ]
  [ take 3 ]
  bi
  llm_answer

# C datalog:
rules [
  (answer Q A) ← (dense Q A) ^ (confidence A C) ^ (C >= 0.7)
  (answer Q A) ← ¬(answer Q _) ^ (decompose Q Qs) ^ (map answer Qs As) ^ (merge As A)
]
```

---

## 4. Приоритетные направления для реализации

| # | Концепт | Что даёт | Сложность | Impact |
|---|---|---|---|---|
| 1 | **APL operators** | Rank-polymorphic tensor ops | Low (Arrow уже есть) | High |
| 2 | **Relational algebra IR** | Rewrite rules, query optimization | Medium | High |
| 3 | **Backus' combining forms** | Формальная композиция пайплайнов | Medium | Medium |
| 4 | **Datalog recursion** | Multi-hop как фиксированная точка | High | High |
| 5 | **Concatenative syntax** | Альтернативная поверхность DSL | Low | Medium |
| 6 | **Charity hylomorphism** | Retrieve → Synthesize formal pattern | Medium | Low-Med |

---

## 5. Источники

- Iverson, K.E. "A Programming Language" (1962)
- Iverson, K.E. "Notation as a Tool for Thought" (Turing Award, 1979)
- Backus, J. "Can Programming Be Liberated from the von Neumann Style?" (1977)
- Codd, E.F. "A Relational Model of Data for Large Shared Data Banks" (1970)
- Cockett & Spencer. "Charity: A Categorical Programming Language" (1992)
- von Thun, M. "Joy: A Concatenative Language" (1990s)
- Schwartz, J. "SETL: A Set-Theoretic Language" (1969)
- Whitney, A. "K: A Programming Language" (1990s)
- Soufflé Datalog: https://souffle-lang.github.io/
- kdb+/q / KDB.AI: https://kx.com/
- Flora-2 / ErgoAI: https://ergoai.org/
- More, T. "NIAL: Nested Interactive Array Language" (1981)
