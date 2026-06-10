# Agentic RAG Evaluation Harness для opencode

## Назначение документа

Этот документ предназначен для передачи агенту opencode как техническая спецификация.

Цель: поручить opencode самостоятельно подготовить агентную инфраструктуру для оценки RAG-системы:

1. Создать специализированных subagents.
2. Создать mock-сценарий для проверки корректности ролей.
3. Протестировать subagents на mock-задаче.
4. После успешного mock-теста создать или обновить `AGENTS.md`.
5. Вписать в `AGENTS.md` методологию проверки настоящего RAG.
6. Подготовить проект к полноценной evaluation pipeline:

```text
Mistral OCR → Markdown → Indexing → Test Generation → Dataset Freeze → RAG Run → Judging → Audit → Report
```

---

# 1. Основная идея

Нужно построить не обычный benchmark-скрипт, а агентный evaluation harness.

opencode должен использовать subagents / child sessions для разделения ролей:

```text
Main Orchestrator
    ├── Indexer Agent
    ├── Chunk Sampler
    ├── Question Generator Agent
    ├── Dataset Freezer
    ├── RAG Runner Agent
    ├── Retrieval Judge
    ├── Answer Judge Agent
    ├── Citation Judge Agent
    └── Auditor Agent
```

Ключевой принцип:

> Каждый агент должен знать только ту информацию, которая нужна его роли.

Это необходимо для защиты от подглядывания, смешивания контекстов и самообмана оценки.

---

# 2. Что должно быть создано

opencode должен создать следующие сущности внутри проекта:

```text
.opencode/
  agents/
    rag-question-generator.md
    rag-runner.md
    rag-answer-judge.md
    rag-citation-judge.md
    rag-auditor.md

  commands/
    rag-eval-mock.md
    rag-eval-generate.md
    rag-eval-run.md
    rag-eval-judge.md
    rag-eval-audit.md
    rag-eval-report.md

eval/
  mock/
    corpus/
    samples/
    datasets/
    runs/

  corpus/
    raw/
    markdown/
    ocr-json/

  index/
    manifest.json

  samples/
    sampled_chunks.jsonl

  datasets/
    draft/
    frozen/

  runs/

scripts/
  mock_rag.py
  sample_chunks.py
  freeze_dataset.py
  retrieval_metrics.py
  build_report.py
```

Если часть директорий уже существует, не удалять их. Только дополнять.

---

# 3. Роли subagents

## 3.1 Main Orchestrator

Главный агент управляет процессом.

Он не должен выполнять все этапы сам. Его задача:

- создавать и вызывать subagents;
- контролировать порядок стадий;
- проверять файловые артефакты;
- запускать mock-тест;
- проверять критерии успеха;
- после успешного mock-теста создать или обновить `AGENTS.md`;
- не перезаписывать frozen datasets и старые runs.

Главное правило:

```text
Orchestrator coordinates.
Subagents execute role-specific semantic tasks.
Scripts execute deterministic tasks.
```

---

## 3.2 rag-question-generator

Файл:

```text
.opencode/agents/rag-question-generator.md
```

Назначение:

- читать sampled source excerpts;
- генерировать вопросы;
- генерировать reference answers;
- указывать expected evidence;
- записывать draft dataset;
- не вызывать RAG;
- не читать runs;
- не читать judge outputs.

Рекомендуемый frontmatter:

```md
---
description: Generates RAG evaluation questions from sampled source excerpts
mode: subagent
temperature: 0.3
permission:
  edit: allow
  bash: deny
  websearch: deny
---

You are the RAG Question Generator.

You receive sampled source excerpts and create evaluation questions.
You must not call the RAG system.
You must not inspect run outputs.
You must not modify frozen datasets.
Write only to eval/datasets/draft/ or eval/mock/datasets/draft/.

For each sample, generate:
- literal question
- paraphrase question
- citation question
- optional negative question
- optional multi-hop question if enough information exists

Every generated test case must include:
- test_id
- document_id
- chunk_id
- question
- reference_answer
- expected_evidence
- question_type
- difficulty
- status: draft
```

---

## 3.3 rag-runner

Файл:

```text
.opencode/agents/rag-runner.md
```

Назначение:

- читать только runner input;
- задавать вопросы RAG;
- сохранять ответы;
- сохранять retrieved contexts;
- не читать source excerpts;
- не читать reference answers;
- не читать expected chunk ids;
- не читать judge notes.

Рекомендуемый frontmatter:

```md
---
description: Runs frozen RAG evaluation questions without access to references
mode: subagent
temperature: 0.0
permission:
  edit: allow
  bash: allow
  websearch: deny
---

You are the RAG Runner.

You may read only runner input files.
You must not read:
- source_excerpt
- reference_answer
- expected_chunk_id
- judge notes
- audit notes

Your task:
- ask each question to the RAG system or mock RAG system
- save the final answer
- save retrieved chunks
- save citations if returned

Write only to the current run directory.
```

Для настоящего RAG runner должен вызывать локальный API, CLI или MCP-инструмент проекта. Для mock-теста runner должен использовать `scripts/mock_rag.py`.

---

## 3.4 rag-answer-judge

Файл:

```text
.opencode/agents/rag-answer-judge.md
```

Назначение:

- оценивать ответы RAG;
- сравнивать их с reference answer;
- проверять retrieved context;
- выявлять галлюцинации;
- выставлять структурированные оценки.

Рекомендуемый frontmatter:

```md
---
description: Judges RAG answers against references and retrieved context
mode: subagent
temperature: 0.1
permission:
  read: allow
  edit: allow
  bash: deny
  websearch: deny
---

You are the RAG Answer Judge.

You evaluate RAG answers using:
- question
- rag_answer
- reference_answer
- source_excerpt
- retrieved_context

You must not modify frozen datasets.
You must not modify raw RAG answers.
You must write judgments only to the current run directory.

For every test case, produce:
- faithfulness: 0.0 to 1.0
- answer_correctness: 0.0 to 1.0
- completeness: 0.0 to 1.0
- hallucination: true/false
- uses_context: true/false
- verdict: pass/fail/partial
- short_reason
```

Особое правило:

> Если ответ правильный по смыслу, но не подтверждается retrieved context, это не полный успех. RAG должен доказывать grounding.

---

## 3.5 rag-citation-judge

Файл:

```text
.opencode/agents/rag-citation-judge.md
```

Назначение:

- проверять цитаты;
- проверять, подтверждают ли цитаты утверждения;
- ловить декоративные или неверные ссылки;
- проверять expected evidence.

Рекомендуемый frontmatter:

```md
---
description: Judges citation correctness and evidence grounding
mode: subagent
temperature: 0.1
permission:
  read: allow
  edit: allow
  bash: deny
  websearch: deny
---

You are the RAG Citation Judge.

You evaluate whether citations support the answer.

Check:
- citations exist when required
- cited chunks are relevant
- cited chunks support the specific answer claims
- expected evidence appears in retrieved or cited chunks
- citations are not decorative

Produce:
- citation_accuracy: 0.0 to 1.0
- expected_evidence_found: true/false
- decorative_citations: true/false
- verdict: pass/fail/partial
- short_reason
```

---

## 3.6 rag-auditor

Файл:

```text
.opencode/agents/rag-auditor.md
```

Назначение:

- проверять качество оценки;
- искать слишком мягкие verdicts;
- искать ошибочно засчитанные ответы;
- выявлять плохие вопросы;
- выявлять неоднозначные reference answers;
- не изменять raw outputs.

Рекомендуемый frontmatter:

```md
---
description: Audits RAG evaluation judgments and detects weak tests or unfair scores
mode: subagent
temperature: 0.2
permission:
  read: allow
  edit: allow
  bash: deny
  websearch: deny
---

You are the RAG Evaluation Auditor.

You do not judge the RAG answer directly.
You audit the quality of the evaluation.

Look for:
- unfair judge verdicts
- overly soft scores
- hallucinations that were missed
- answers marked correct without evidence
- bad or ambiguous test questions
- weak reference answers
- leakage between generator and runner
- corrupted or incomplete run artifacts

Write an audit report to the current run directory.
Do not overwrite raw judge outputs.
```

---

# 4. Формат тестового dataset

Dataset должен храниться в JSONL.

Одна строка — один test case:

```json
{
  "test_id": "doc_001_q_001",
  "document_id": "doc_001",
  "source_file": "mock_doc_001.md",
  "chunk_id": "doc_001_chunk_001",
  "source_excerpt": "Система повторяет сетевой запрос не более трёх раз при временной ошибке соединения.",
  "question": "Сколько раз система повторяет сетевой запрос при временной ошибке соединения?",
  "reference_answer": "Система повторяет сетевой запрос не более трёх раз.",
  "question_type": "literal",
  "difficulty": "easy",
  "expected_evidence": ["doc_001_chunk_001"],
  "must_include": ["не более трёх раз"],
  "must_not_include": [],
  "status": "draft"
}
```

После freeze:

```json
{
  "status": "frozen"
}
```

Frozen dataset нельзя редактировать. Для изменений создаётся новая версия:

```text
rag_eval_v001.jsonl
rag_eval_v002.jsonl
rag_eval_v003.jsonl
```

---

# 5. Формат runner input

Runner не должен видеть reference answers и source excerpts.

Для него нужно создавать отдельный файл:

```json
{
  "test_id": "doc_001_q_001",
  "question": "Сколько раз система повторяет сетевой запрос при временной ошибке соединения?"
}
```

Файл:

```text
eval/datasets/frozen/<dataset_id>/runner_input.jsonl
```

или для mock:

```text
eval/mock/datasets/frozen/<dataset_id>/runner_input.jsonl
```

---

# 6. Формат run outputs

Каждый запуск должен создавать новую директорию:

```text
eval/runs/run_YYYY_MM_DD_NNN/
```

или для mock:

```text
eval/mock/runs/run_YYYY_MM_DD_NNN/
```

Внутри:

```text
run_manifest.json
rag_answers.jsonl
retrieved_contexts.jsonl
retrieval_metrics.json
answer_judgments.jsonl
citation_judgments.jsonl
audit_report.md
final_report.md
summary.json
```

Пример `run_manifest.json`:

```json
{
  "run_id": "run_2026_06_10_001",
  "dataset_id": "rag_eval_v001",
  "rag_version": "mock_rag_v1",
  "embedding_model": "mock",
  "reranker_model": "none",
  "chunking_strategy": "mock_chunks",
  "top_k": 3,
  "temperature": 0,
  "ocr_pipeline": "mistral_ocr_markdown_v1",
  "started_at": "2026-06-10T00:00:00Z",
  "finished_at": null
}
```

---

# 7. Mock-тест перед настоящим RAG

Перед созданием финального `AGENTS.md` opencode должен протестировать subagents на mock-задаче.

## 7.1 Mock corpus

Создать минимум 3 mock-документа:

```text
eval/mock/corpus/mock_doc_001.md
eval/mock/corpus/mock_doc_002.md
eval/mock/corpus/mock_doc_003.md
```

Пример содержания:

```md
# Mock Document 001: Network Retry Policy

Система повторяет сетевой запрос не более трёх раз при временной ошибке соединения.
После третьей неудачной попытки задача переводится в состояние failed_retry_limit.
Для постоянных ошибок авторизации повторная отправка не выполняется.
```

```md
# Mock Document 002: Queue Memory Limit

Очередь задач хранит не более 500 активных элементов.
Если лимит очереди достигнут, новые задачи получают статус rejected_queue_full.
Очередь очищается после успешной обработки или окончательной ошибки.
```

```md
# Mock Document 003: Citation Policy

Каждый ответ RAG должен содержать ссылку на chunk, из которого был получен ключевой факт.
Если в документах нет ответа, система должна честно сообщить, что информации недостаточно.
Декоративные цитаты считаются ошибкой.
```

---

## 7.2 Mock RAG

Создать `scripts/mock_rag.py`.

Mock RAG должен:

- принимать вопрос;
- искать простым keyword matching по mock corpus;
- возвращать ответ;
- возвращать retrieved chunks;
- возвращать citations.

Это не должен быть умный RAG. Его задача — проверить pipeline.

Пример поведения:

Вопрос:

```text
Сколько раз система повторяет сетевой запрос?
```

Ответ:

```json
{
  "answer": "Система повторяет сетевой запрос не более трёх раз.",
  "retrieved_chunks": [
    {
      "document_id": "mock_doc_001",
      "chunk_id": "mock_doc_001_chunk_001",
      "rank": 1,
      "score": 1.0,
      "text": "Система повторяет сетевой запрос не более трёх раз при временной ошибке соединения."
    }
  ],
  "citations": ["mock_doc_001_chunk_001"]
}
```

---

## 7.3 Mock evaluation flow

opencode должен выполнить:

```text
1. Create mock corpus.
2. Create subagents.
3. Generate sampled chunks from mock corpus.
4. Invoke rag-question-generator.
5. Freeze mock dataset.
6. Create runner_input without references.
7. Invoke rag-runner with mock RAG.
8. Compute retrieval metrics.
9. Invoke rag-answer-judge.
10. Invoke rag-citation-judge.
11. Invoke rag-auditor.
12. Build final mock report.
13. Verify success criteria.
14. Only after success: create/update AGENTS.md.
```

---

# 8. Mock success criteria

Mock-тест считается успешным, если:

```text
- Созданы все required subagents.
- Созданы все required commands.
- Создан mock corpus.
- Создан draft dataset.
- Создан frozen dataset.
- Runner input не содержит source_excerpt, reference_answer, expected_evidence.
- RAG Runner не использовал reference answers.
- Есть rag_answers.jsonl.
- Есть retrieved_contexts.jsonl.
- Есть retrieval_metrics.json.
- Есть answer_judgments.jsonl.
- Есть citation_judgments.jsonl.
- Есть audit_report.md.
- Есть final_report.md.
- Retrieval Hit@1 на literal mock questions >= 0.8.
- Citation Accuracy на citation mock questions >= 0.8.
- Unanswerable question должен получить честный отказ или "недостаточно информации".
```

Если критерии не выполнены:

- не создавать финальный `AGENTS.md`;
- создать `eval/mock/runs/<run_id>/failure_report.md`;
- указать, что нужно исправить;
- повторить mock-тест после исправлений.

---

# 9. Что должно быть записано в AGENTS.md после успешного mock-теста

После успешного mock-теста opencode должен создать или обновить `AGENTS.md`.

`AGENTS.md` должен содержать:

## 9.1 Project evaluation protocol

```md
# RAG Evaluation Protocol

This project uses an agentic RAG evaluation harness.

The evaluation flow is:

1. OCR documents with Mistral OCR.
2. Store Markdown and OCR JSON.
3. Index Markdown into the RAG system.
4. Sample chunks.
5. Generate draft questions with isolated subagents.
6. Freeze the dataset.
7. Run RAG using runner input only.
8. Compute retrieval metrics.
9. Judge answer quality.
10. Judge citation quality.
11. Audit judge outputs.
12. Build final report.
```

## 9.2 Role isolation rules

```md
## Role Isolation Rules

- Question Generator may read source excerpts.
- Question Generator must not call the RAG system.
- RAG Runner may read only runner input.
- RAG Runner must not read source excerpts, reference answers, expected evidence, or judge notes.
- Judges may read references and run outputs.
- Judges must not modify frozen datasets or raw RAG answers.
- Auditor may read all evaluation outputs.
- Auditor must not overwrite raw judge outputs.
- Frozen datasets are append-only.
- Every run must create a new run directory.
```

## 9.3 Dataset rules

```md
## Dataset Rules

- Draft datasets are editable.
- Frozen datasets are immutable.
- To change tests, create a new dataset version.
- Store datasets in JSONL.
- Store human-readable reports in Markdown.
- Keep runner input separate from full dataset.
```

## 9.4 Metrics

```md
## Required Metrics

Retrieval:
- Hit@1
- Hit@3
- Hit@5
- MRR
- DocumentHit@k
- ChunkHit@k

Generation:
- Faithfulness
- Answer Correctness
- Completeness
- Hallucination Rate
- Context Usage

Citation:
- Citation Accuracy
- Expected Evidence Found
- Decorative Citation Detection

Negative Questions:
- Unanswerable Accuracy
```

## 9.5 Mistral OCR rules

```md
## Mistral OCR Rules

- Use Mistral OCR to extract documents to Markdown.
- Preserve OCR JSON when available.
- Store Markdown in eval/corpus/markdown/.
- Store OCR JSON in eval/corpus/ocr-json/.
- Preserve page boundaries when possible.
- Preserve tables and headings.
- Keep OCR metadata in document manifest.
```

## 9.6 Run logging rules

```md
## Run Logging Rules

Every run must include:
- run_id
- dataset_id
- RAG version
- OCR pipeline version
- embedding model
- reranker model
- chunking strategy
- top_k
- generation temperature
- started_at
- finished_at
```

---

# 10. Настоящая RAG evaluation pipeline

После успешного mock-теста и создания `AGENTS.md` opencode должен быть готов проверить настоящий RAG.

Настоящий запуск должен использовать:

```text
eval/corpus/raw/
eval/corpus/markdown/
eval/corpus/ocr-json/
```

Ожидаемый процесс:

```text
1. Пользователь помещает документы в eval/corpus/raw/.
2. OCR pipeline извлекает Markdown и OCR JSON.
3. Markdown индексируется в RAG.
4. sample_chunks.py выбирает chunks.
5. rag-question-generator создаёт draft dataset.
6. freeze_dataset.py создаёт frozen dataset и runner_input.
7. rag-runner задаёт вопросы настоящему RAG.
8. retrieval_metrics.py считает retrieval metrics.
9. answer judge оценивает качество ответов.
10. citation judge оценивает цитаты.
11. auditor проверяет качество оценки.
12. build_report.py создаёт final_report.md и summary.json.
```

---

# 11. Команды opencode

Желательно создать команды:

```text
/rag-eval-mock
/rag-eval-generate
/rag-eval-run
/rag-eval-judge
/rag-eval-audit
/rag-eval-report
```

## 11.1 rag-eval-mock

Команда должна запускать полный mock-test pipeline.

```md
---
description: Run full mock test for the agentic RAG evaluation harness
agent: build
subtask: false
---

Run the full mock evaluation pipeline.

Steps:
1. Create required directories.
2. Create or verify subagents.
3. Create mock corpus.
4. Generate mock samples.
5. Invoke rag-question-generator.
6. Freeze dataset.
7. Invoke rag-runner against scripts/mock_rag.py.
8. Compute retrieval metrics.
9. Invoke rag-answer-judge.
10. Invoke rag-citation-judge.
11. Invoke rag-auditor.
12. Build mock final report.
13. Check success criteria.
14. If successful, create/update AGENTS.md with the final methodology.
15. If failed, write failure_report.md and do not update AGENTS.md.
```

---

## 11.2 rag-eval-generate

```md
---
description: Generate a draft RAG evaluation dataset
agent: rag-question-generator
subtask: true
---

Generate a draft RAG evaluation dataset from sampled chunks.

Dataset name: $1
Input samples: @eval/samples/sampled_chunks.jsonl
Output: eval/datasets/draft/$1.jsonl

Do not call the RAG system.
Do not read any run outputs.
```

---

## 11.3 rag-eval-run

```md
---
description: Run frozen RAG evaluation questions
agent: rag-runner
subtask: true
---

Run RAG evaluation.

Dataset: $1
Run ID: $2

Read only:
eval/datasets/frozen/$1/runner_input.jsonl

Write to:
eval/runs/$2/

Do not read full dataset with references.
Do not read source excerpts.
Do not read judge outputs.
```

---

## 11.4 rag-eval-judge

```md
---
description: Judge RAG answer quality
agent: rag-answer-judge
subtask: true
---

Judge answer quality for run: $1

Use:
- frozen dataset references
- rag answers
- retrieved contexts

Write:
eval/runs/$1/answer_judgments.jsonl

Do not modify frozen datasets.
Do not modify raw RAG answers.
```

---

## 11.5 rag-eval-audit

```md
---
description: Audit RAG evaluation judgments
agent: rag-auditor
subtask: true
---

Audit the evaluation run: $1

Read:
- answer judgments
- citation judgments
- retrieval metrics
- final report if present

Write:
eval/runs/$1/audit_report.md

Do not overwrite raw outputs.
```

---

# 12. Важные запреты

opencode должен соблюдать следующие запреты:

```text
- Do not let RAG Runner read reference answers.
- Do not let Question Generator call RAG.
- Do not let Judges modify frozen datasets.
- Do not overwrite previous runs.
- Do not overwrite frozen datasets.
- Do not use one agent/session for all semantic roles.
- Do not mix mock and real evaluation outputs.
- Do not report success without checking artifacts.
- Do not hide failed cases.
```

---

# 13. Финальная цель

После выполнения этой спецификации проект должен получить:

```text
1. Набор opencode subagents для RAG evaluation.
2. Набор opencode commands для стадий evaluation.
3. Mock-tested evaluation harness.
4. AGENTS.md с методологией.
5. Структуру директорий для настоящих запусков.
6. Возможность запускать regression evaluation после изменений RAG.
```

Главная формула:

```text
Generator creates the exam.
Runner takes the exam.
Judges grade the exam.
Auditor checks the graders.
Orchestrator preserves the evidence.
```

---

# 14. Критерий готовности

Работа считается завершённой, когда opencode может показать:

```text
- созданные .opencode/agents/*.md
- созданные .opencode/commands/*.md
- успешный mock final_report.md
- успешный audit_report.md
- созданный или обновлённый AGENTS.md
- отсутствие утечки reference answers в runner_input.jsonl
```

Только после этого можно переходить к проверке настоящего RAG.
