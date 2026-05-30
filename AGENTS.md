# OpenCode Support Protocol for Solo Agents

## 1. Core Rule

The primary agent is the owner of the task.

`opencode` is an external, read-only worker pool. Use it to gather facts, run checks, review ideas, and draft supporting text. Do not treat it as a second coder with shared memory or project authority.

The primary agent owns:

- final architecture and technical decisions
- production code edits
- test selection and interpretation
- final documentation wording
- user-facing conclusions

`opencode` may help, but its output is evidence, not truth. Verify anything that affects code, architecture, tests, or documentation.

## 2. When to Use OpenCode

Use `opencode` when the work is:

- context-heavy or repetitive
- easy to parallelize
- useful as an independent review
- likely to take more than 5-10 minutes
- blocked by uncertainty where a second opinion helps
- mostly read-only: research, logs, tests, docs, git history, call-site mapping

Do not use `opencode` just to satisfy a ritual. For tiny tasks, direct inspection is fine.

## 3. What to Delegate

Good tasks for `opencode`:

- codebase research and call-site mapping
- log analysis and failure summarization
- running specified tests and reporting exact output
- independent diff review
- git history inspection: `log`, `blame`, `bisect` support
- environment validation
- web research against current official documentation
- documentation drafts and walkthrough drafts

Do not delegate:

- production source edits
- bug fixes in project files
- refactors
- database or data migrations
- final architecture decisions
- cross-file coordination
- tasks where partial or stale context can corrupt the result

Documentation rule: `opencode` may draft documentation, but the primary agent must verify it against the actual diff, commands, logs, and current project state before accepting it.

## 4. OpenCode Roles

Use one focused role per `opencode` call.

### Scout

Find relevant files, functions, call sites, configs, and risks.

Prompt shape:

```text
You are Scout. Inspect the repository for [topic].
Return relevant files, call sites, assumptions, and risks.
Do not modify any files.
```

### Tester

Run explicit commands and report exact results.

Prompt shape:

```text
You are Tester. Run these commands:
[commands]
Return exit codes, key output, and failures.
Do not modify any files.
```

### Reviewer

Review a diff or implementation for bugs, regressions, missing tests, and unsafe assumptions.

Prompt shape:

```text
You are Reviewer. Review the current diff for [goal].
Prioritize correctness bugs, regressions, and missing tests.
Return findings with file paths and line references where possible.
Do not modify any files.
```

### Researcher

Find current official documentation or best practices.

Prompt shape:

```text
You are Researcher. Find the latest official documentation as of 2026-05-30 for [topic].
Ignore deprecated approaches.
Return links, short summary, and recommended implementation constraints.
Do not modify any files.
```

### Scribe

Draft documentation or a walkthrough from facts provided by the primary agent.

Prompt shape:

```text
You are Scribe. Draft documentation for [change].
Use only the facts below. Mark unknowns explicitly.
[facts]
Write to stdout unless a specific docs path is explicitly allowed.
Do not modify source files.
```

## 5. Required Output Contract

Every `opencode` result should be easy to audit. Ask it to return:

```text
Summary:
Evidence:
Files inspected:
Commands run:
Risks or uncertainty:
Recommendation:
```

For tests, require exact command lines and exit codes.

For web research, require source links and dates when available.

For reviews, require severity labels:

- P0: data loss, security issue, crash, or unusable core workflow
- P1: likely correctness bug or major regression
- P2: edge-case bug, missing validation, or meaningful test gap
- P3: maintainability or clarity issue

## 6. Session Tracking

Use `.brains/sessions/` for complex or multi-phase work.

Session directory format:

```text
.brains/sessions/YYYY-MM-DD-NNNN/
  plans/
  tasks/
  walkthrough/
```

Create a session when the task has multiple phases, multiple agents, or a non-trivial verification trail.

Minimum contents:

- `plans/`: what will be changed or investigated
- `tasks/`: checklist with `[ ]`, `[/]`, `[x]`
- `walkthrough/`: final summary after verification

Small tasks do not need a session.

## 7. Parallel Dispatch

Run independent support tasks in parallel, then continue primary work while they run.

Rules:

- one agent = one atomic task
- prefer 1-3 parallel agents
- redirect output to log files
- read and verify logs before using their conclusions
- include `Do not modify any files.` unless a docs-only write is explicitly allowed

Linux/macOS:

```bash
OPENCODE_SERVER_PASSWORD="" OPENCODE_SERVER_USERNAME="" opencode run --pure "Scout task. Do not modify any files." > /tmp/scout.log 2>&1 &
OPENCODE_SERVER_PASSWORD="" OPENCODE_SERVER_USERNAME="" opencode run --pure "Tester task. Do not modify any files." > /tmp/tester.log 2>&1 &
wait
```

PowerShell:

```powershell
$env:OPENCODE_SERVER_PASSWORD = ""
$env:OPENCODE_SERVER_USERNAME = ""
$root = (Get-Location).Path
$j1 = Start-Job -ScriptBlock { param($d) Set-Location $d; opencode run --pure "Scout task. Do not modify any files." } -ArgumentList $root
$j2 = Start-Job -ScriptBlock { param($d) Set-Location $d; opencode run --pure "Tester task. Do not modify any files." } -ArgumentList $root
$j1, $j2 | Wait-Job | Receive-Job
```

## 8. Verification Rules

The primary agent must verify:

- files that `opencode` claims are relevant
- tests that `opencode` claims passed or failed
- generated documentation against real diffs and logs
- web research against official or primary sources when possible
- any recommendation before implementing it

If `opencode` gives vague output, ask a narrower follow-up or discard it.

If `opencode` disagrees with local evidence, local evidence wins.

## 9. Model Selection

Use models by task type:

- documentation and summaries: `opencode/deepseek-v4-flash-free` or another fast/free text model
- code analysis and debugging hypotheses: stronger reasoning model when available
- simple lookups: any reliable free model

Do not rely on model names staying stable. If a named model is unavailable, choose the closest available model for the role.

## 10. Platform Notes

### Windows PowerShell

If `OPENCODE_SERVER_PASSWORD` is set by OpenCode Desktop, `opencode run` may fail with `Session not found`. Clear both variables before running:

```powershell
$env:OPENCODE_SERVER_PASSWORD = ""
$env:OPENCODE_SERVER_USERNAME = ""
opencode run --pure "Your task. Do not modify any files."
```

Optional wrapper:

```powershell
function opencode-run {
  $env:OPENCODE_SERVER_PASSWORD = ""
  $env:OPENCODE_SERVER_USERNAME = ""
  opencode run @args
}
```

### Linux/macOS

Optional wrapper:

```bash
opencode-run() {
  OPENCODE_SERVER_PASSWORD="" OPENCODE_SERVER_USERNAME="" opencode run "$@"
}
```

## 11. Anti-Patterns

Avoid:

- letting `opencode` edit production code
- asking one agent to solve five unrelated tasks
- accepting summaries without evidence
- using web results without checking recency and source quality
- creating a session folder for trivial work
- waiting idly when independent work can continue
- treating `opencode` as persistent memory

## 12. Dataset Lessons

After a complex task, `opencode` may draft a lesson for later training data.

Storage:

```text
.dataset/lessons/
```

Schema:

```json
{
  "topic": "Short descriptive topic",
  "instruction": "The initial problem statement",
  "context": "Codebase state before the fix",
  "response": "Detailed solution with before/after code"
}
```

Rules:

- write in English
- use snake_case file names
- primary agent must read and verify the JSON before accepting it
- do not include secrets, private tokens, or unrelated user data
