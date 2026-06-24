# Active Disambiguation Layer with MCP Skills

> Status: design proposal / experimental feature  
> Scope: optional client-side behavior layer for Tensor Machine MCP integrations  
> Core idea: use Tensor Machine search statistics and ambiguity signals to let an LLM ask Akinator-like clarification questions through MCP skills/prompts

## 1. Purpose

This document describes an optional active disambiguation layer for Tensor Machine.

The goal is to avoid forcing the retrieval engine to become an autonomous agent. Tensor Machine should remain a retrieval engine exposed through MCP tools. The language model or MCP client can provide the conversational behavior through skills, prompts, and tool-use policies.

In simple terms:

```text
Tensor Machine detects ambiguity.
LLM generates a short clarifying question.
User answers.
Tensor Machine runs a narrowed search.
Statistics are updated.
```

This is similar in spirit to Akinator, but without a handcrafted global question database. Instead, the LLM generates clarification questions from live retrieval hypotheses and search statistics.

## 2. Motivation

A RAG system often fails not because retrieval is weak, but because the user query is ambiguous.

Example query:

```text
How do I configure memory?
```

Possible meanings:

- agent conversation memory;
- smart-cache retrieval memory;
- vector database storage;
- session memory;
- tenant-level memory;
- model context window management.

A normal RAG engine may guess. An active disambiguation layer should ask one useful question when the cost of guessing is higher than the cost of clarification.

## 3. Architectural principle

Do not turn the RAG core into an agent.

Recommended separation:

```text
Tensor Machine MCP Server
  -> retrieves documents
  -> computes scores
  -> detects ambiguity
  -> exposes tools/resources/prompts
  -> records statistics

LLM / MCP Client
  -> reads ambiguity payloads
  -> follows skills/prompts
  -> asks human-friendly questions
  -> calls Tensor Machine again with refined intent
```

Tensor Machine should not be responsible for producing long conversational behavior. It should provide enough structured context for the LLM client to decide whether clarification is needed.

## 4. Relation to MCP

The feature can be implemented as a combination of:

- MCP tools;
- MCP prompts;
- MCP resources;
- client-side skills / tool-use policies.

Tensor Machine can expose tools such as:

```text
search
search_text
read_document
get_document_metadata
analyze_ambiguity
refine_search
record_clarification_feedback
```

It can also expose prompts or skill-like instructions such as:

```text
rag_disambiguation_skill
rag_refinement_skill
rag_feedback_skill
rag_uncertainty_skill
```

The LLM remains the natural-language generator. Tensor Machine remains the retrieval and scoring engine.

## 5. Akinator-like behavior

Classic Akinator requires a large handcrafted or learned question-answer database.

In this architecture, the LLM can generate questions dynamically from:

- top competing retrieval hypotheses;
- intent clusters;
- smart-cache candidates;
- learned cluster memory;
- top-k score gaps;
- query statistics;
- tenant-specific retrieval patterns;
- known document categories;
- repeated clarification patterns.

The system does not need a static global question tree. It needs structured ambiguity signals and a skill that tells the LLM how to ask one useful question.

## 6. Ambiguity signals

Tensor Machine may return ambiguity metadata together with search results.

Potential signals:

```text
top1_score
top2_score
score_gap
entropy_like_distribution
number_of_intent_clusters
competing_cluster_ids
conflicting_retrieval_channels
smart_cache_conflict
low_confidence_reason
suggested_question_target
```

Example ambiguity payload:

```json
{
  "status": "ambiguous",
  "confidence": 0.42,
  "score_gap": 0.03,
  "top_hypotheses": [
    {
      "intent": "smart_cache_memory",
      "confidence": 0.31
    },
    {
      "intent": "agent_memory",
      "confidence": 0.28
    },
    {
      "intent": "vector_storage",
      "confidence": 0.24
    }
  ],
  "recommended_action": "ask_clarifying_question",
  "question_target": "memory_type"
}
```

The LLM can then ask:

```text
Do you mean retrieval memory, agent behavior memory, or vector storage?
```

## 7. When to ask a question

The active disambiguation layer should not ask questions constantly.

It should ask only when clarification is likely to improve retrieval quality.

Recommended triggers:

```text
- low top-1 confidence;
- small top-1 / top-2 score gap;
- several strong but different intent clusters;
- conflict between dense and BM25 channels;
- conflict between smart-cache and source-grounded retrieval;
- query is too short or underspecified;
- previous similar queries often required clarification;
- multiple document categories match equally well.
```

A simple rule:

```text
if cost_of_wrong_answer > cost_of_clarification:
    ask one question
else:
    answer directly
```

## 8. Question generation policy

The LLM should not ask arbitrary questions.

The skill should instruct it to:

- ask at most one clarification question at a time;
- keep it short;
- avoid exposing internal implementation details;
- present options when possible;
- prefer questions that separate the strongest competing hypotheses;
- avoid asking when confidence is already high;
- avoid creating a long interrogation flow.

Bad question:

```text
The RSF score distribution has low margin between the SmartCache and Entity channels. Which one should I use?
```

Good question:

```text
Do you mean project architecture, setup/configuration, or debugging an error?
```

## 9. MCP skill drafts

### 9.1 rag_disambiguation_skill

Purpose:

```text
If Tensor Machine returns status=ambiguous, do not answer immediately. Ask one short question that best separates the top hypotheses.
```

Behavior:

```text
- Use top_hypotheses and question_target.
- Ask one user-facing question.
- Do not mention RSF, smart-cache, score gaps, or internal cluster IDs unless the user is technical and explicitly asks.
- Prefer 2-4 answer options.
```

### 9.2 rag_refinement_skill

Purpose:

```text
After the user answers a clarification question, convert the answer into a refined MCP search request.
```

Behavior:

```text
- Preserve original query.
- Add clarified intent.
- Add narrowed filters or route hints when available.
- Call refine_search or search with the refined parameters.
```

### 9.3 rag_feedback_skill

Purpose:

```text
If smart-cache or learned cluster memory participated in retrieval, ask a lightweight usefulness question only when appropriate.
```

Behavior:

```text
- Ask naturally.
- Do not expose internal memory mechanics.
- Record yes/no/ignored outcomes through MCP when possible.
```

Example:

```text
Did this find what you needed?
```

### 9.4 rag_uncertainty_skill

Purpose:

```text
Prefer clarification over confident guessing when ambiguity is high.
```

Behavior:

```text
- If ambiguity is high, ask.
- If ambiguity is moderate but answer can safely include assumptions, answer with the assumption stated.
- If ambiguity is low, do not ask.
```

## 10. Tool interface draft

### analyze_ambiguity

Input:

```json
{
  "query": "string",
  "tenant_id": "string",
  "session_id": "string | null",
  "top_k": 10
}
```

Output:

```json
{
  "status": "clear | ambiguous | low_confidence",
  "confidence": 0.0,
  "top_hypotheses": [],
  "question_target": "string | null",
  "recommended_action": "answer | ask_clarifying_question | search_more"
}
```

### refine_search

Input:

```json
{
  "original_query": "string",
  "clarification": "string",
  "tenant_id": "string",
  "session_id": "string | null",
  "route_hint": "string | null"
}
```

Output:

```json
{
  "results": [],
  "confidence": 0.0,
  "used_clarification": true,
  "updated_route_signature": "string"
}
```

### record_clarification_feedback

Input:

```json
{
  "query_id": "string",
  "clarification_question": "string",
  "user_answer": "string",
  "result_helpful": true,
  "tenant_id": "string"
}
```

Output:

```json
{
  "recorded": true
}
```

## 11. Statistics used by the layer

A mature RAG system should already collect useful operational statistics. The active disambiguation layer can reuse them.

Useful statistics:

```text
query frequency
intent cluster frequency
document/chunk selection frequency
source open/click/read frequency
failed search frequency
clarification question frequency
clarification success rate
route signature success rate
top-k score distribution
score gap distribution
smart-cache activation frequency
learned cluster activation frequency
```

These statistics can help the system learn which questions are worth asking.

## 12. Learning behavior

This layer can be considered learning because it changes future clarification and search behavior based on previous outcomes.

It does not train the LLM.

It learns:

- which ambiguities are common;
- which questions reduce uncertainty;
- which clarified intents lead to successful retrieval;
- which route hints are useful;
- when not to ask.

Short formula:

```text
retrieval ambiguity -> generated clarification -> refined search -> outcome statistics -> better future clarification
```

## 13. Relation to adaptive cluster memory

Adaptive Cluster Memory learns successful search routes.

Active Disambiguation learns when and how to narrow the user's intent before or during search.

They should work together:

```text
Adaptive Cluster Memory
  -> remembers successful retrieval traces

Active Disambiguation Layer
  -> asks when traces/hypotheses conflict or confidence is low

RSF
  -> fuses retrieval signals after intent is clear enough
```

Smart-cache and learned clusters can provide signals for question generation, but they should not force a clarification unless uncertainty is high.

## 14. Configuration

This feature must be optional and configurable.

Recommended default:

```env
ACTIVE_DISAMBIGUATION_ENABLED=false
```

Suggested settings:

```env
ACTIVE_DISAMBIGUATION_ENABLED=false
ACTIVE_DISAMBIGUATION_MAX_QUESTIONS=1
ACTIVE_DISAMBIGUATION_MIN_CONFIDENCE=0.50
ACTIVE_DISAMBIGUATION_MIN_SCORE_GAP=0.08
ACTIVE_DISAMBIGUATION_USE_SMART_CACHE_SIGNALS=true
ACTIVE_DISAMBIGUATION_USE_CLUSTER_SIGNALS=true
ACTIVE_DISAMBIGUATION_RECORD_STATS=true
ACTIVE_DISAMBIGUATION_TENANT_ISOLATION=true
ACTIVE_DISAMBIGUATION_PROMPT_NAME=rag_disambiguation_skill
```

The feature should be safe to disable. If disabled, Tensor Machine should return normal search results without asking the client to clarify.

## 15. Safety and UX constraints

Potential risks:

- asking too many questions;
- exposing internal mechanics;
- overfitting to tenant-specific habits;
- turning simple queries into annoying flows;
- learning bad clarification patterns;
- leaking tenant-specific intent patterns.

Mitigations:

- disabled by default;
- max one question per ambiguous turn;
- tenant isolation;
- no internal jargon in user-facing questions;
- record clarification success rate;
- decay old statistics;
- let clients opt out.

## 16. Implementation phases

### Phase 1: ambiguity metadata only

- Return confidence, score gap, and top hypotheses.
- Do not ask questions automatically.
- Add logs and metrics.

### Phase 2: MCP prompt / skill support

- Add `rag_disambiguation_skill` prompt.
- Add client guidance for when to ask.
- Keep behavior client-side.

### Phase 3: refinement loop

- Add `refine_search`.
- Preserve original query and clarification.
- Track route signatures.

### Phase 4: statistics-based question quality

- Record clarification outcomes.
- Track which question types reduce ambiguity.
- Use statistics to improve future question targets.

### Phase 5: integration with adaptive memory

- Use smart-cache and learned cluster conflicts as ambiguity signals.
- Keep memory influence weak and optional.
- Ensure tenant isolation.

## 17. Summary

The active disambiguation layer lets Tensor Machine avoid guessing when the search space is ambiguous.

The LLM is the right component for generating human-friendly questions.

Tensor Machine should expose structured ambiguity signals and MCP skills/prompts. The MCP client should decide how to ask the user.

Final formula:

```text
search ambiguity -> MCP skill -> LLM clarification -> refined search -> stored statistics -> better future retrieval
```

Key rule:

> Tensor Machine should remain a retrieval engine; the conversational clarification behavior should live in the MCP client skill layer.
