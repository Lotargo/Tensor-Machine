# Detailed Fix Plan — Source Quality Improvement

Date: 2026-06-09
Based on: Regression test analysis (docs/regression_analysis.md)

## Root Cause Analysis

Regression test revealed: **all 6 queries found the correct document, but 3/5 War and Peace queries had weak or no source grounding.**

### Primary bottleneck: 2-chunk-per-doc cap

In `response.rs:160-168`:
```rust
if !explicit_structured_lookup && unique_chunks.len() > 2 {
    // ... sort by rank/score ...
    unique_chunks.truncate(2);  // <-- HERE
}
```

For non-structured queries (which is the default), **each document is limited to 2 chunks**. In a 600-page novel with ~2000 chunks, this means:
- Q3 (Tushin): retrieval found the farewell scene (1 chunk), but the battle/reprimand scene (different chunk) was truncated
- Q4 (Anatole): retrieval found the Bourienne household scene, but the courtship scene was truncated
- Q5 (Pratzen): retrieval found unrelated scenes; the actual sky moment was in a different chunk that didn't make the top-2

### Secondary bottleneck: No neighbor expansion for regular queries

In `response.rs:79-83`:
```rust
let max_span_chunks = if explicit_structured_lookup {
    indices.len().saturating_mul(4).clamp(8, 40)
} else {
    0  // <-- neighbor expansion DISABLED for regular queries
};
```

`fetch_structured_span_details` is only called for explicit structured lookups. Regular queries get no neighboring chunks.

---

## Proposed Fixes (ordered by impact/effort)

### Fix 1: Increase chunk cap from 2 to 3 (LOW effort, HIGH impact)

**File:** `response.rs:160`
**Change:** `truncate(2)` → `truncate(3)`

**Why:** Adds 1 more chunk per document. For Q3/Q4, the third chunk would likely contain the missing context (battle scene, courtship scene). Minimal token budget impact.

**Risk:** Low. Token limit (`MCP_SEARCH_MAX_RESPONSE_TOKENS=5000`) already handles overflow.

---

### Fix 2: Enable neighbor expansion for all queries (MEDIUM effort, HIGH impact)

**File:** `response.rs:79-83`
**Change:** Remove the `explicit_structured_lookup` guard — always expand neighbors.

```rust
let max_span_chunks = indices.len().saturating_mul(2).clamp(4, 20);
```

**Why:** When retrieval finds chunk N, neighboring chunks (N-1, N+1) often contain the surrounding context. For Q5 (Pratzen sky), the relevant passage might be 1-2 chunks away from the retrieved chunks.

**Risk:** Medium. Increases DB queries and token usage. Need to verify `fetch_structured_span_details` handles the general case correctly.

---

### Fix 3: Adaptive chunk cap based on document size (MEDIUM effort, MEDIUM impact)

**File:** `response.rs:160`
**Change:** Instead of hard-coded `2`, use a cap that scales with document length.

```rust
let cap = if doc_chunk_count > 500 { 4 } else if doc_chunk_count > 100 { 3 } else { 2 };
```

**Why:** Short documents (like "Утро") work fine with 2 chunks. Long novels need more.

**Risk:** Medium. Requires knowing `doc_chunk_count` at response-building time.

---

### Fix 4: Score-proximity re-ranking (HIGH effort, MEDIUM impact)

**Concept:** After initial retrieval, boost chunks that are adjacent (by chunk_index) to high-scoring chunks.

**Why:** If chunk 500 has score 0.7 and chunk 501 has score 0.3, chunk 501 is likely part of the same scene and should be boosted.

**Risk:** High effort, moderate gain. Deferred.

---

### Fix 5: Scene-aware chunk selection (HIGH effort, HIGH impact)

**Concept:** Use `chapter_id` and `header_chain` metadata to identify scene boundaries. When multiple chunks from the same chapter are retrieved, prefer chunks from the same scene.

**Why:** Would solve Q3/Q4/Q5 by selecting chunks from the correct scene within the document.

**Risk:** High effort. Requires scene boundary detection during ingestion. Deferred.

---

## Recommended Implementation Order

1. **Fix 1** (truncate 2→3) — immediate, minimal risk
2. **Fix 2** (neighbor expansion) — requires testing but high impact
3. **Fix 3** (adaptive cap) — if Fix 1 is insufficient
4. Fixes 4-5 — future work

## Expected Outcome

After Fix 1+2:
- Q3 (Tushin): should retrieve both farewell AND battle scenes → FULLY GROUNDED
- Q4 (Anatole): should retrieve both household AND courtship scenes → FULLY GROUNDED
- Q5 (Pratzen): may still struggle (famous scene, model knows it anyway) → PARTIAL improvement
- Q1, Q2, Q6: already working → no change
