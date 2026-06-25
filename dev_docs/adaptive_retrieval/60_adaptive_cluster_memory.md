# Adaptive Cluster Memory for Retrieval

> Status: design proposal / experimental feature  
> Scope: optional retrieval-layer extension for Tensor Machine  
> Core idea: smart-cache traces + TTL + weak RSF influence + cluster learning over successful retrieval patterns

## 1. Purpose

This document describes an optional adaptive retrieval-memory mechanism for Tensor Machine.

The feature is intended to make a narrow-domain RAG system gradually improve its retrieval behavior without fine-tuning the LLM and without treating generated answers as permanent truth.

The system should not learn by rewriting model weights. Instead, it should learn by remembering successful retrieval situations as weak, temporary, confidence-gated search signals.

In simple terms:

```text
query
  -> normal retrieval
  -> optional smart-cache lookup
  -> optional learned cluster memory
  -> RSF fusion
  -> answer
  -> weak feedback / TTL update
```

The goal is not to replace source-grounded retrieval. The goal is to add a cautious memory layer that helps the engine remember useful search routes.

## 2. Motivation

Classic RAG often treats each query as isolated:

```text
query -> vector search / keyword search -> top-k -> answer
```

Tensor Machine already improves this with multiple retrieval signals:

- dense semantic search;
- BM25 / lexical search;
- entity search;
- structural metadata;
- clustering signals;
- RSF fusion.

The proposed extension adds another weak signal:

- smart-cache / adaptive cluster memory.

This memory should capture repeated successful retrieval patterns, not final answers as absolute knowledge.

The system should learn that:

> Similar queries in this tenant/domain often benefit from this search route, this document region, these entities, or this cluster.

## 3. Important principle: optional by default

This feature must be optional.

It should be possible to:

- enable it;
- disable it completely;
- tune its influence;
- tune TTL behavior;
- tune confidence thresholds;
- isolate it per tenant;
- run the system safely without it.

Recommended default:

```text
ADAPTIVE_CLUSTER_MEMORY_ENABLED=false
```

The core RAG engine must not depend on this feature. If the adaptive memory layer is disabled, Tensor Machine should behave as a normal retrieval engine with its existing RSF pipeline.

## 4. Conceptual model

The feature consists of three layers:

```text
Smart Cache Candidate
        ↓
Confidence-Gated Memory Entry
        ↓
Learned Retrieval Cluster
```

### 4.1 Smart Cache Candidate

A smart-cache candidate is created only after a similar retrieval pattern appears multiple times.

Example rule:

```text
if similar query/retrieval pattern appears 2-3 times:
    create smart-cache candidate with low confidence and short TTL
```

A candidate should not strongly affect search. It should only become a weak RSF signal.

### 4.2 Confidence-Gated Memory Entry

A smart-cache candidate becomes more useful only if it participates in retrieval and receives positive or non-negative evidence.

Evidence sources:

- explicit user confirmation;
- repeated successful reuse;
- lack of negative signal;
- repeated similarity across narrow-domain queries.

Negative feedback should kill or quarantine the entry.

### 4.3 Learned Retrieval Cluster

When enough related smart-cache entries exist, the system may cluster them into a learned retrieval pattern.

The cluster is not a topic cluster only. It is a cluster of retrieval experience.

A learned cluster may represent:

- similar user intents;
- similar document regions;
- similar entities;
- similar successful routes;
- similar query-answer-source patterns;
- similar tenant/domain behavior.

## 5. Why this is learning

This mechanism can be considered learning at the retrieval layer because the system changes future search behavior based on previous search outcomes.

It is not LLM fine-tuning.

It is not supervised model training.

It is closer to:

- adaptive retrieval memory;
- implicit relevance feedback;
- contextual bandit-style reinforcement;
- online/batch cluster learning;
- search-route optimization.

Short formula:

```text
successful search traces -> smart cache -> clusters -> weak RSF boost -> better future retrieval
```

The system learns the map of search routes, not the language model itself.

## 6. Smart-cache lifecycle

Recommended lifecycle:

```text
observed
  -> candidate
  -> weak
  -> trusted
  -> expired / rejected / quarantined
```

### observed

A similar query or retrieval pattern appeared once.

No cache entry should be created yet, or the entry should remain internal and inactive.

### candidate

A similar pattern appeared 2-3 times.

The system creates a weak temporary entry.

Properties:

- low confidence;
- short TTL;
- very weak RSF influence;
- tenant-scoped;
- not directly inserted into answers.

### weak

The candidate participated in retrieval and was not rejected.

It may slightly influence RSF.

### trusted

The entry has multiple positive confirmations or repeated successful activations.

It may receive a stronger but still bounded RSF weight.

Even trusted entries must not override primary document evidence.

### expired

TTL ended. The entry is ignored or deleted.

### rejected

User explicitly said the result was wrong or not useful.

The entry should be deleted or moved to quarantine.

### quarantined

A suspicious entry is kept for analysis but does not participate in retrieval.

## 7. Feedback model

A heavy judge model is intentionally avoided.

Human feedback is useful but not always reliable. Agent feedback is also risky if the agent knows the cache participated in retrieval.

Recommended approach:

When a smart-cache candidate participated in retrieval, the agent may ask a lightweight question after the answer:

```text
Did this find what you needed?
```

Possible outcomes:

```text
yes      -> confidence increases, TTL extends significantly
no       -> entry is deleted or quarantined
ignored  -> TTL may extend slightly, confidence increases minimally or not at all
```

Important rule:

> Silence is not a strong positive signal.

Ignored feedback should not be treated as confirmation. It may only slightly extend TTL if the system wants to avoid losing potentially useful weak memory too quickly.

## 8. RSF integration

Adaptive memory must enter retrieval as a weak signal, not as a final answer.

Suggested RSF channels:

```text
Dense
BM25
Entity
Structural
Recency
Cluster
SmartCache
LearnedClusterMemory
```

The memory channels should have small default weights.

Example starting range:

```text
RSF_SMART_CACHE_WEIGHT=0.03
RSF_LEARNED_CLUSTER_WEIGHT=0.05
```

These values should be configurable and bounded.

The smart-cache result should boost candidates only when other retrieval channels do not strongly disagree.

Recommended principle:

> Adaptive memory may whisper, but it must not shout.

## 9. HDBSCAN / clustering layer

HDBSCAN may be used to group successful retrieval traces into dense regions of repeated behavior.

This is useful because the system should not promote isolated one-off events into long-term memory.

Potential vector representation for each retrieval trace:

```text
trace_vector = combine(
    query_embedding,
    answer_summary_embedding,
    retrieved_source_embeddings,
    entity_features,
    route_features,
    tenant/domain metadata
)
```

Potential cluster metadata:

```text
cluster_id
tenant_id
centroid / representative vector
member_count
positive_count
negative_count
confidence
created_at
updated_at
expires_at
source_doc_ids
source_entity_ids
preferred_route
rsf_boost
```

HDBSCAN should not run on every query. It should run in batches:

```text
- after N new traces;
- or every X hours;
- or during maintenance jobs;
- or manually through an admin/debug command.
```

## 10. Multitenant architecture

This feature is especially useful for narrow-domain and multitenant RAG systems.

However, tenant isolation is mandatory.

Recommended layers:

```text
user_memory      -> personal search behavior
session_memory   -> temporary session-level behavior
tenant_memory    -> team/company/project behavior
domain_memory    -> optional anonymized domain-level behavior
global_memory    -> disabled by default
```

Rules:

- tenant A must not directly influence tenant B;
- private source IDs must not leak across tenants;
- domain-level clusters should only use anonymized or aggregated patterns;
- per-tenant disable switches must exist;
- memory deletion must be possible.

## 11. Suggested configuration

The feature should be controlled through configuration.

Recommended environment variables:

```env
# Master switch
ADAPTIVE_CLUSTER_MEMORY_ENABLED=false

# Smart cache
SMART_CACHE_ENABLED=false
SMART_CACHE_MIN_HITS=3
SMART_CACHE_INITIAL_TTL_HOURS=72
SMART_CACHE_MAX_TTL_HOURS=720
SMART_CACHE_POSITIVE_TTL_EXTENSION_HOURS=168
SMART_CACHE_IGNORED_TTL_EXTENSION_HOURS=12
SMART_CACHE_NEGATIVE_ACTION=quarantine
SMART_CACHE_MIN_CONFIDENCE_TO_ACTIVATE=0.35
SMART_CACHE_MAX_RSF_WEIGHT=0.07

# Feedback
SMART_CACHE_FEEDBACK_PROMPT_ENABLED=true
SMART_CACHE_FEEDBACK_ON_PARTICIPATION_ONLY=true
SMART_CACHE_IGNORED_CONFIDENCE_DELTA=0.0
SMART_CACHE_POSITIVE_CONFIDENCE_DELTA=0.15
SMART_CACHE_NEGATIVE_CONFIDENCE_DELTA=-1.0

# Learned cluster memory
LEARNED_CLUSTER_MEMORY_ENABLED=false
LEARNED_CLUSTER_MIN_MEMBERS=5
LEARNED_CLUSTER_MIN_CONFIDENCE=0.55
LEARNED_CLUSTER_REBUILD_INTERVAL_MINUTES=60
LEARNED_CLUSTER_MAX_RSF_WEIGHT=0.10
LEARNED_CLUSTER_TTL_HOURS=720

# HDBSCAN-like settings
CLUSTER_MEMORY_MIN_CLUSTER_SIZE=5
CLUSTER_MEMORY_MIN_SAMPLES=2
CLUSTER_MEMORY_DISTANCE_THRESHOLD=0.25

# Multitenancy
CLUSTER_MEMORY_TENANT_ISOLATION=true
CLUSTER_MEMORY_DOMAIN_SHARED_ENABLED=false
```

Config names may be changed during implementation, but the master principle must remain:

```text
the feature must be optional, configurable, and safe to disable
```

## 12. Data model draft

### retrieval_memory_entries

```text
id
tenant_id
user_id nullable
session_id nullable
query_text
query_embedding_ref
answer_summary nullable
answer_embedding_ref nullable
source_doc_ids
source_chunk_ids
entity_ids
route_signature
confidence
hit_count
positive_count
negative_count
ignored_count
status
created_at
updated_at
expires_at
metadata
```

### retrieval_memory_clusters

```text
id
tenant_id
cluster_label nullable
representative_embedding_ref
member_count
confidence
rsf_boost
preferred_route_signature nullable
source_doc_ids
entity_ids
created_at
updated_at
expires_at
status
metadata
```

### retrieval_memory_events

```text
id
memory_entry_id nullable
cluster_id nullable
tenant_id
query_id
participated_in_search
rank_before nullable
rank_after nullable
feedback_type nullable
confidence_delta
ttl_delta
created_at
metadata
```

## 13. Activation logic draft

A simplified query-time flow:

```text
1. Run normal retrieval candidate generation.
2. If adaptive memory is disabled, skip all memory logic.
3. Search smart-cache entries by query embedding and tenant_id.
4. Keep only active entries with sufficient confidence and valid TTL.
5. Convert matched memory entries into weak scores.
6. Search learned clusters if enabled.
7. Convert cluster matches into weak scores.
8. Add SmartCache / LearnedClusterMemory as RSF channels.
9. Produce final ranking.
10. If memory participated, optionally ask lightweight feedback question.
11. Update confidence and TTL according to feedback.
```

## 14. Safety and failure modes

Potential risks:

- reinforcing wrong answers;
- clustering repeated mistakes;
- tenant leakage;
- old memory overriding fresh documents;
- feedback spam;
- overfitting to one user;
- memory bloat.

Mitigations:

- feature disabled by default;
- low RSF weight;
- TTL decay;
- negative feedback kills/quarantines entries;
- tenant isolation;
- source-grounded retrieval remains primary;
- max memory size limits;
- observability metrics;
- admin/debug visibility.

## 15. Observability

Recommended metrics:

```text
smart_cache_candidates_created_total
smart_cache_activations_total
smart_cache_positive_feedback_total
smart_cache_negative_feedback_total
smart_cache_expired_total
smart_cache_quarantined_total
learned_clusters_created_total
learned_cluster_activations_total
adaptive_memory_rsf_weight_avg
adaptive_memory_rank_delta_avg
```

Recommended traces:

```text
memory_lookup
memory_activation
memory_rsf_fusion
memory_feedback_update
cluster_rebuild
cluster_decay
```

## 16. Implementation phases

### Phase 1: disabled-by-default smart-cache

- Add config switches.
- Add data model.
- Create candidate entries after repeated similar retrieval patterns.
- Add TTL and confidence fields.
- Do not affect RSF yet.

### Phase 2: weak RSF signal

- Add SmartCache as optional RSF channel.
- Keep very low default weight.
- Log rank changes.

### Phase 3: feedback-aware TTL

- Ask lightweight feedback only when smart-cache participated.
- Positive feedback increases confidence and TTL.
- Negative feedback deletes/quarantines.
- Ignored feedback produces minimal or zero confidence growth.

### Phase 4: learned cluster memory

- Batch-cluster successful smart-cache traces.
- Add LearnedClusterMemory as optional RSF channel.
- Keep tenant isolation strict.

### Phase 5: operational hardening

- Add admin controls.
- Add metrics.
- Add cleanup jobs.
- Add memory export/debug view.
- Add per-tenant policies.

## 17. Summary

This feature turns repeated successful retrieval behavior into weak, temporary, configurable memory.

It does not make the LLM learn.

It makes the retrieval layer learn where and how to search.

Final formula:

```text
smart-cache -> TTL/confidence -> HDBSCAN clusters -> weak RSF signal -> adaptive retrieval behavior
```

Key rule:

> Adaptive memory must support retrieval, not replace evidence.
