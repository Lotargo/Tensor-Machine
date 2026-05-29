# OpenCode CLI Agent Collaboration Guidelines

## 1. Core Architecture

You (the primary agent) are the **sole architect and coder**. The `opencode` CLI agents are your **support crew** — they research, test, document, and advise, but **NEVER write production code**.

**Why:** `opencode` runs smaller models in isolated, stateless sessions with no inter-agent communication. They cannot coordinate with each other, share context, or safely merge concurrent edits. You are the single source of truth.

## 2. Session Tracking

All sessions are tracked in `.brains/sessions/` using the format `YYYY-MM-DD-NNNN` (e.g., `2026-05-29-0001`).

Each session directory contains:

```
.brains/sessions/2026-05-29-0001/
├── plans/          # Detailed implementation plan (created BEFORE work begins)
├── tasks/          # Checklist with progress marks: [ ] → [/] → [x]
└── walkthrough/    # Post-completion summary of what was done and why
```

**Workflow:**

1. Create the session directory and write the plan **before** delegating any work.
2. Update `tasks/` as you progress.
3. Write the walkthrough **after** verification is complete.

Agents can reference the plan file in their prompts for context alignment.

## 3. What Agents Do (and Don't)

**Delegate to `opencode`:**

- Codebase research and analysis
- Web search for docs, APIs, and solutions
- Running tests and reporting results
- Generating documentation
- Git history analysis (bisect, blame, log)
- Environment setup and validation

**NEVER delegate to `opencode`:**

- Writing or modifying source code
- Architectural decisions
- Fixing bugs in production files
- Refactoring
- Database migrations
- Anything requiring cross-file coordination

## 4. Parallel Dispatch Pattern

Fire off support agents in parallel with `&`, then continue your own coding work. Review their output when notifications arrive.

```bash
# Research + Test + Doc agents in parallel while you code
opencode run --pure "Analyze src/core.rs: list all exported functions, their args, and call sites. DO NOT modify any files." > /tmp/research_core.log 2>&1 &
opencode run --pure "Run: cargo test. Report all failures with full output. DO NOT modify any files." > /tmp/test_results.log 2>&1 &
opencode run --pure "Search the web for '[relevant technical topic]'. Summarize findings. DO NOT modify any files." > /tmp/web_research.log 2>&1 &
wait
# Now read /tmp/*.log and incorporate findings into your work
```

**Key rules:**

- Every agent prompt MUST include `DO NOT modify any files` (except doc writers targeting `docs/`).
- Always redirect output to `/tmp/` log files for later review.
- Use `wait` only when you need results before proceeding. Otherwise, let them run and check later.

## 5. Brainstorming & Consultation

When you hit a wall — a confusing bug, an unclear API, or a design dilemma — **consult `opencode` before asking the user**.

```bash
# Stuck on a bug? Ask for hypotheses.
opencode run --pure "I'm debugging [file]. [Description of the problem]. Here is the relevant code: [paste snippet]. What could cause this? List 3-5 hypotheses ranked by likelihood. DO NOT modify any files."

# Need a second opinion on architecture?
opencode run --pure "I'm considering two approaches for [task]: (A) [approach A], (B) [approach B]. What are the trade-offs? DO NOT modify any files."
```

**When to escalate to the user:** Only after you've consulted an agent AND still lack clarity on a business/product decision (not a technical one).

## 6. Delegate-First Rule

You MUST delegate research, web lookups, test runs, and codebase analysis to `opencode` agents **before** doing it yourself. Do your own research only when an agent's output was incorrect or insufficient.

**Rationale:** Your context window is expensive. Preserve it for architecture and coding. Let agents burn their disposable contexts on information gathering.

When delegating web research, always instruct agents to verify that documentation and solutions are **current as of today's date**. Outdated patterns from previous years are a common failure mode. Include phrasing like: "Find the latest official documentation as of [current year]. Ignore deprecated approaches."

## 7. Model Selection

- For **documentation generation** and text-heavy tasks, use `DeepSeek V4 Flash Free` or `MiMo V2.5 Free`.
- For **code analysis and debugging hypotheses**, use `Big Pickle`.
- For **simple lookups and web search summaries**, use any free model.

## 8. Common Patterns

**Git detective — find where a bug was introduced:**

```bash
opencode run --pure "Run 'git log --oneline -20 -- [file]' and 'git diff HEAD~5 -- [file]'. Identify which commit likely introduced the [issue]. DO NOT modify any files." > /tmp/git_detective.log 2>&1 &
```

**Prep environment while you plan:**

```bash
opencode run --pure "Check that [required tools] are installed and accessible from PATH. Report versions. If any are missing, list installation commands for this OS. DO NOT modify any files." > /tmp/env_check.log 2>&1 &
```

**Multi-agent research swarm:**

```bash
opencode run --pure "Search the web for '[topic A]'. Summarize with code examples. DO NOT modify any files." > /tmp/web_topic_a.log 2>&1 &
opencode run --pure "Search the web for '[topic B]'. Summarize with code examples. DO NOT modify any files." > /tmp/web_topic_b.log 2>&1 &
opencode run --pure "Analyze [file]: explain what each function does. DO NOT modify any files." > /tmp/research_file.log 2>&1 &
wait
```

## 9. Anti-Patterns

- ❌ Letting an agent "fix" code it found during research
- ❌ Skipping the plan — always write `.brains/sessions/*/plans/` first
- ❌ Sending a huge prompt with 5 tasks — one agent = one atomic task
- ❌ Forgetting `DO NOT modify any files` in research/test prompts
- ❌ Waiting idly for agents — code while they work

## 10. Platform-Specific Notes

### Windows (PowerShell 5.1)

**Known issue:** If `OPENCODE_SERVER_PASSWORD` is set (e.g., by OpenCode Desktop), `opencode run` fails with `Session not found` ([bug #24747](https://github.com/anomalyco/opencode/issues/24747)). Always clear the variable before calling:

```powershell
$env:OPENCODE_SERVER_PASSWORD=""; $env:OPENCODE_SERVER_USERNAME=""
opencode run --model "opencode/deepseek-v4-flash-free" "Your prompt here"
```

PowerShell 5.1 does not support `&` for background jobs. Use `Start-Job` instead:

```powershell
$env:OPENCODE_SERVER_PASSWORD=""; $env:OPENCODE_SERVER_USERNAME=""
$j1 = Start-Job -ScriptBlock { param($d) Set-Location $d; opencode run --model "opencode/deepseek-v4-flash-free" "Task 1. DO NOT modify any files." } -ArgumentList (Get-Location).Path
$j2 = Start-Job -ScriptBlock { param($d) Set-Location $d; opencode run --model "opencode/deepseek-v4-flash-free" "Task 2. DO NOT modify any files." } -ArgumentList (Get-Location).Path
$j1, $j2 | Wait-Job | Receive-Job
```

For convenience, create a wrapper function in your PowerShell profile:

```powershell
function opencode-run { $env:OPENCODE_SERVER_PASSWORD=""; $env:OPENCODE_SERVER_USERNAME=""; opencode run @args }
```

### Linux / macOS (bash / zsh)

The same `OPENCODE_SERVER_PASSWORD` issue applies. Use a wrapper or clear the variable inline:

```bash
# Wrapper function (add to ~/.bashrc or ~/.zshrc)
opencode-run() {
  OPENCODE_SERVER_PASSWORD="" OPENCODE_SERVER_USERNAME="" opencode run "$@"
}

# One-liner with env prefix
OPENCODE_SERVER_PASSWORD="" OPENCODE_SERVER_USERNAME="" opencode run --model "opencode/deepseek-v4-flash-free" "Your prompt here" &
```

Parallel dispatch with `&` works natively:

```bash
OPENCODE_SERVER_PASSWORD="" opencode run --pure "Task 1" > /tmp/t1.log 2>&1 &
OPENCODE_SERVER_PASSWORD="" opencode run --pure "Task 2" > /tmp/t2.log 2>&1 &
wait
```

## 11. Automated Dataset Generation

After completing a complex task, delegate lesson creation to `opencode`:

- **Storage:** `.dataset/lessons/` with snake_case `.json` names.
- **Language:** English only.
- **Schema:**
  ```json
  {
    "topic": "Short descriptive topic",
    "instruction": "The initial problem statement",
    "context": "Codebase state before the fix",
    "response": "Detailed solution with before/after code"
  }
  ```
- **After generation:** You MUST read and verify the JSON file yourself.

```bash
opencode run --pure --model "deepseek v4 flash free" "Generate an AI training lesson in JSON format about [topic]. Write in English. Save to .dataset/lessons/[name].json using the schema from AGENTS.md."
```
