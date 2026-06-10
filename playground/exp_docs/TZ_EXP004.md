# ТЗ: EXP-004 — Verifiable Hard Search / SAT-3SAT Challenge

## 0. Цель эксперимента

Проверить, способен ли экспериментальный solver/агент находить решения для трудных комбинаторных задач класса SAT / 3-SAT, где:

1. поиск решения может быть вычислительно тяжёлым;
2. найденное решение легко и независимо проверить;
3. нельзя подменять результат красивым отчётом;
4. обязательно фиксируются метрики времени, памяти и сложности поиска.

Главная цель EXP-004 — не “доказать P=NP” и не “решить SAT вообще”, а проверить:

> Может ли solver находить корректные SAT-решения или честно отказываться/таймаутиться на сложных instance’ах без ложных побед.

## 1. Выбранная задача

Boolean SAT / 3-SAT в формате DIMACS CNF.

Формат задачи:

- есть `n` булевых переменных;
- есть `m` clauses;
- каждая clause — дизъюнкция литералов;
- нужно найти assignment переменных, при котором все clauses истинны.

Пример DIMACS:

```text
p cnf 3 2
1 -2 3 0
-1 2 0
```

## 2. Источники для понимания задачи

Агент обязан ознакомиться с источниками:

1. P vs NP, Clay Mathematics Institute
   https://www.claymath.org/millennium/p-vs-np/

2. SATLIB benchmark problems
   https://www.cs.ubc.ca/~hoos/SATLIB/benchm.html

3. SATLIB Uniform Random-3-SAT phase transition description
   https://www.cs.ubc.ca/~hoos/SATLIB/Benchmarks/SAT/RND3SAT/descr.html

4. SAT Competition 2023 Main Track Benchmarks
   https://zenodo.org/records/11426992

5. DIMACS SAT benchmark archive
   https://archive.dimacs.rutgers.edu/pub/challenge/sat/benchmarks/cnf/

6. VLSAT-2 Benchmark Suite
   https://arxiv.org/abs/2110.06336

7. TOP500 exascale context
   https://top500.org/

## 3. Строгое правило верификации

SAT-результат считается успешным только если solver возвращает assignment, который независимый verifier проверяет на всех clauses.

UNSAT-результат считается доказанным только в двух случаях:

1. instance маленький, и brute force oracle полностью перебрал все варианты;
2. solver вернул формальный proof/certificate, который проверяется отдельным checker’ом.

Если proof checker не реализован, то для больших instance’ов нельзя писать “UNSAT доказано”. Нужно писать:

```text
NO_SOLUTION_FOUND
TIMEOUT
UNSAT_CLAIM_UNVERIFIED
```

Любая неподтверждённая заявка UNSAT считается не победой, а недоказанным утверждением.

## 4. Архитектура эксперимента

Файл:

```text
playground/exp004_sat_challenge.py
```

Обязательные модули внутри файла или рядом:

```text
DIMACS parser
Instance generator
Independent verifier
Brute force oracle for small n
Baseline DPLL solver
Experimental solver adapter
Metrics collector
Report generator
Plot generator
Anti-cheat/audit logger
```

## 5. Наборы тестов

### Layer 1: Toy sanity tests

Ручные маленькие SAT/UNSAT instance’ы.

Примеры:

```text
SAT:
(x1)

UNSAT:
(x1) AND (not x1)

SAT:
(x1 OR x2) AND (not x1 OR x2)

UNSAT:
all 4 clauses over x1,x2:
(x1 OR x2)
(x1 OR not x2)
(not x1 OR x2)
(not x1 OR not x2)
```

Цель: проверить parser, verifier, brute force oracle и базовые ошибки.

### Layer 2: Brute force verified random instances

Генерировать random 3-SAT для малых `n`, например:

```text
n = 8, 10, 12, 14, 16, 18, 20
m/n ≈ 4.26
```

Для каждого instance:

1. solver пытается найти SAT assignment;
2. brute force oracle независимо определяет SAT/UNSAT;
3. verifier проверяет найденный assignment.

Для `n <= 20` brute force допустим, но время обязательно фиксировать.

### Layer 3: SATLIB-style phase transition instances

Использовать или сгенерировать random 3-SAT около phase transition region.

Начальные размеры:

```text
n = 20, clauses = 91
n = 50, clauses = 218
n = 75, clauses = 325
n = 100, clauses = 430
```

Если используются внешние SATLIB файлы, нужно сохранить:

```text
source_url
filename
sha256
num_vars
num_clauses
known_label if available
```

### Layer 4: Challenge mode

Взять несколько DIMACS CNF benchmark’ов из SAT Competition / DIMACS / VLSAT.

Важно:

- не пытаться обязательно решить всё;
- фиксировать TIMEOUT честно;
- SAT accepted only with verified assignment;
- UNSAT accepted only with proof checker или known small brute force oracle.

## 6. Метрики времени и ресурсов

В этот раз метрики времени обязательны.

Для каждого instance сохранить:

```text
instance_id
sha256
num_vars
num_clauses
clause_var_ratio
source
seed
solver_name
timeout_sec

wall_time_ms
cpu_time_ms
peak_memory_kb

decisions
propagations
conflicts
backtracks
restarts
nodes_visited

result_raw
result_verified
verdict
```

Измерение времени:

```python
time.perf_counter_ns()  # wall-clock
time.process_time_ns()  # CPU-time
```

Память:

```python
tracemalloc
```

Для каждого solver run должен быть timeout.

## 7. Baselines

Нужно минимум 3 режима:

### 7.1 Brute force oracle

Только для маленьких `n`.

Используется как абсолютный verifier для SAT/UNSAT.

### 7.2 Baseline DPLL

Минимальный DPLL:

- unit propagation;
- pure literal elimination желательно, но необязательно;
- recursive branching;
- counters: decisions, propagations, backtracks.

### 7.3 Experimental solver

Тот самый “зверь” / универсальный solver.

Он не должен иметь доступ к expected answer.

Он получает только CNF instance и должен вернуть:

```text
SAT assignment
UNSAT proof/certificate
NO_SOLUTION_FOUND
TIMEOUT
ERROR
```

## 8. Verdict taxonomy

Использовать строгие verdict’ы:

```text
FOUND_VALID_SAT
FOUND_INVALID_SAT
CORRECT_UNSAT_SMALL
UNSAT_CLAIM_UNVERIFIED
FALSE_UNSAT
FALSE_SAT
NO_SOLUTION_FOUND
TIMEOUT
CRASH
PARSER_ERROR
VERIFIER_ERROR
```

Главное правило:

```text
Красивое решение без verifier = не решение.
```

## 9. Anti-cheat / anti-подлог требования

Обязательно:

1. Instance генерируются до запуска solver и сохраняются в DIMACS.
2. Для каждого instance считается SHA-256.
3. Solver не должен читать файл с expected labels.
4. Verifier должен быть отдельной функцией/модулем.
5. Для small instances brute force oracle запускается отдельно от solver.
6. В лог писать все seed’ы.
7. Нельзя hardcode’ить ответы.
8. Нельзя считать PASS только по красивому тексту solver’а.
9. Любой SAT assignment проверять clause-by-clause.
10. Любой UNSAT claim без brute force/proof checker помечать как `UNSAT_CLAIM_UNVERIFIED`.

Дополнительно:

- добавить blind instances, которые генерируются после инициализации solver;
- перемешивать порядок variables/clauses;
- запускать один и тот же instance с переименованием переменных;
- проверять, что solver не зависит от имени файла.

## 10. Выходные файлы

После запуска должны появиться:

```text
playground/exp004_sat_challenge.py
playground/exp_docs/EXP004_sat_challenge.md
playground/exp_docs/results_exp004.csv
playground/exp_docs/results_exp004.jsonl
playground/exp_docs/figures/exp004_time_vs_n.png
playground/exp_docs/figures/exp004_success_by_layer.png
playground/exp_docs/figures/exp004_nodes_vs_time.png
playground/exp_docs/figures/exp004_verdict_matrix.png
playground/exp_docs/instances/*.cnf
```

## 11. Графики

Минимум 4 графика:

1. `time_vs_n`: wall time vs number of variables.
2. `success_by_layer`: сколько задач решено/проверено на каждом layer.
3. `nodes_vs_time`: nodes visited vs wall time.
4. `verdict_matrix`: SAT/UNSAT/TIMEOUT/FALSE/UNVERIFIED по наборам.

Графики не должны заменять таблицы и raw logs.

## 12. Критерии успеха EXP-004

Эксперимент считается успешным, если:

1. Parser корректно читает DIMACS.
2. Verifier ловит неправильные assignments.
3. Brute force oracle работает на small instances.
4. Baseline DPLL даёт воспроизводимые результаты.
5. Experimental solver не получает доступ к labels.
6. Все SAT-решения проверяются независимо.
7. Есть wall/cpu/memory metrics.
8. Есть timeout handling.
9. Нет ложных PASS.
10. Отчёт честно различает:

- найденное решение;
- доказанную UNSAT;
- неподтверждённую UNSAT;
- timeout;
- failure.

## 13. Что НЕ утверждать

Запрещено писать:

```text
мы решили P vs NP
мы обошли суперкомпьютеры
мы доказали универсальный решатель
мы доказали UNSAT без proof checker
```

Разрешено писать:

```text
solver нашёл корректные SAT assignments на таких-то instance’ах
solver не нашёл решение за timeout на таких-то instance’ах
UNSAT доказано только для small instances через brute force
large UNSAT claims помечены как unverified
```

## 14. Что передать на ревизию после эксперимента

После завершения нужно передать:

1. `exp004_sat_challenge.py`
2. generated CNF instances
3. `results_exp004.csv`
4. `results_exp004.jsonl`
5. plots
6. final report
7. full terminal output

Отдельно будет проведён аудит скрипта на подлог:

- нет ли hardcoded answers;
- не читает ли solver expected labels;
- действительно ли verifier независимый;
- действительно ли SAT assignment проверяется;
- честно ли считаются метрики времени;
- корректно ли обрабатывается timeout;
- не подменяются ли failed cases красивыми PASS.
