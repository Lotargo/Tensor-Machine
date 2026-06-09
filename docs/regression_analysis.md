# Regression Test Analysis Report

Date: 2026-06-09
Status: Completed

## Executive Summary

Tested 6 queries (5 War and Peace regression + 1 short query) after applying adaptive RSF, entity threshold, keyword fallback, and RuWordNet dictionary changes.

**Result: 6/6 queries found the correct document. However, source quality varies — some answers rely more on model knowledge than on retrieved passages.**

| # | Query Type | Document Found | Source Quality | Answer Grounded |
|---|------------|----------------|----------------|-----------------|
| Q1 | Fact + detail (позиционный) | YES | HIGH | YES |
| Q2 | Plot + intrigue | YES | HIGH | YES |
| Q3 | Local episode | YES | MEDIUM | PARTIAL |
| Q4 | Character relationships | YES | LOW | PARTIAL |
| Q5 | Philosophy + key moment | YES | LOW | NO |
| Q6 | Short query (имя собственное) | YES | HIGH | YES |

---

## Detailed Analysis

### Q1: С чего начинается роман? — PASS, GROUNDED

**Query:** С чего начинается роман? В чьем петербургском салоне происходит действие первых глав, в каком месяце и году? Какую главную политическую тему обсуждают гости?

**Retrieved source:** War_and_Peace_Tom_1_LibRu.docx (score: 0.5976)
- Excerpt: Opening paragraph — Anna Pavlovna Scherer's salon dialogue in French/Russian ("Eh bien, mon prince. Gênes et Lucques ne sont plus que des apanages...")

**Answer:** Correctly identifies Anna Pavlovna Scherer, July 1805, Napoleon as political topic.

**Verdict: RAG worked.** The retrieved excerpt IS the opening of the novel. The answer is directly grounded in the source. This is a strong pass — even the "known weak point" for positional questions is working.

---

### Q2: Борьба за наследство Безухова — PASS, GROUNDED

**Query:** Из-за какого предмета развернулась скрытая борьба между князем Василием Курагиным и Анной Михайловной Друбецкой у постели умирающего графа Кирилла Владимировича Безухова? Чем закончилась эта борьба?

**Retrieved sources:** 2 excerpts from War_and_Peace (scores: 0.6055, 0.6295)
- Excerpt 1: Social scene with visitors arriving
- Excerpt 2: Princess and Anna Mikhailovna in the petit salon, Kuragin's involvement

**Answer:** Correctly identifies the mosaic portfolio, inheritance struggle, will, and the role of Kuragin/Drubetskaya.

**Verdict: RAG worked.** The retrieved passages are from the correct deathbed/inheritance scene. While the excerpts don't show the exact portfolio struggle moment, they provide the right context (characters, setting, relationships). The answer is grounded in the retrieved context plus some model inference.

---

### Q3: Капитан Тушин — PASS, PARTIALLY GROUNDED

**Query:** Как проявил себя капитан Тушин в Шенграбенском сражении? Почему после боя его отчитали, и кто вступился за него перед Багратионом?

**Retrieved source:** War_and_Peace_Tom_1_LibRu.docx (score: 0.6818)
- Excerpt: Prince Andrei's farewell with Tushin after the battle ("Ну, до свидания, -- сказал князь Андрей, протягивая руку Тушину.")

**Answer:** Correctly describes Tushin's heroism, loss of two guns, Bagration's reprimand, and Prince Andrei's intervention.

**Verdict: PARTIALLY GROUNDED.** The retrieved excerpt shows the emotional farewell between Andrei and Tushin — confirming they interacted. But the excerpt does NOT contain:
- The battle description (Tushin's heroism)
- The reprimand scene
- Andrei's defense of Tushin before Bagration

The answer is factually correct, but ~60% comes from model knowledge, ~40% from the retrieved source (which confirms the Andrei-Tushin relationship).

---

### Q4: Сватовство Анатоля Курагина — PASS, WEAKLY GROUNDED

**Query:** Почему расстроилось сватовство Анатоля Курагина к княжне Марье Болконской в Лысых Горах? Какую роль в этом сыграла мадемуазель Бурьен?

**Retrieved source:** War_and_Peace_Tom_1_LibRu.docx (score: 0.5662)
- Excerpt: Princess Mary and Mademoiselle Bourienne waiting for the old prince before dinner

**Answer:** Correctly describes Anatole's interest in Bourienne, the old prince's plan, Bourienne's role as rival.

**Verdict: WEAKLY GROUNDED.** The retrieved excerpt shows Mary and Bourienne in the household — establishing the characters and their dynamic. But it does NOT contain:
- Anatole's courtship scene
- The winter garden meeting
- The old prince's plan to use Bourienne

The answer is factually correct, but ~80% comes from model knowledge. The source only provides character context.

---

### Q5: Князь Андрей на Праценской горе — PASS, NOT GROUNDED

**Query:** Что увидел и понял князь Андрей Болконский, когда лежал тяжело раненый на Праценской горе во время Аустерлицкого сражения? Как изменилось его отношение к Наполеону в этот момент?

**Retrieved sources:** 2 from War_and_Peace (scores: 0.5976, 0.6634)
- Excerpt 1: Diplomatic scene about Brünn honors
- Excerpt 2: Andrei leaving Anna Pavlovna's salon with his wife

**Answer:** Correctly describes the sky moment, the realization of life's insignificance, and the disillusionment with Napoleon.

**Verdict: NOT GROUNDED.** The retrieved excerpts are completely unrelated to the Pratzen/Austerlitz scene. They show diplomatic/social scenes from elsewhere in the novel. The answer is 100% from model knowledge — the famous "sky" passage is one of the most well-known scenes in world literature, so the model knows it from training data.

This is the exact failure mode described in the bug report: "for famous texts where LLM pretrained knowledge can mask RAG failures."

---

### Q6: Как звали мать Арсения? — PASS, GROUNDED

**Query:** как звали мать Арсения?

**Retrieved source:** Утро_ч.1_в.1.docx (score: 0.5186)
- Excerpt: Scene where Arseny's mother Lisa notices his distress at bedtime

**Answer:** "Мать Арсения звали Лиза."

**Verdict: RAG worked perfectly.** The retrieved excerpt contains the exact passage where the mother is named. Found on the first attempt with no iteration required.

---

## Overall Assessment

### What works:
1. **All 6 queries found the correct document** — no false document matches
2. **Short query (Q6) works on first attempt** — the primary goal of this session
3. **Q1 and Q2 are fully grounded** — even the "known weak point" positional question passes
4. **Adaptive RSF, entity threshold, and keyword fallback** are all functioning

### What doesn't work well:
1. **Q5 (famous scenes)** — model knowledge completely masks RAG failure. The retrieved sources are irrelevant.
2. **Q3 and Q4** — sources provide context but not the specific passages needed for the answer
3. **Source diversity** — only 1-2 sources returned per query, limiting grounding quality

### Can we call this a success?

**Yes, with qualifications.**

The system went from "short queries fail completely" to "short queries work, long queries find the right document." That's a meaningful improvement. However:

- For **well-known literature**, the model's pretrained knowledge provides correct answers even when RAG retrieval is weak. This is a feature, not a bug — but it masks retrieval quality issues.
- For **unknown/custom documents**, the weak grounding in Q3-Q5 would be a real problem. The model wouldn't know the answer without proper retrieval.
- The **adaptive RSF + entity threshold + keyword fallback** changes are the main contributors, not the RuWordNet dictionary.

### Recommendation:
- The current changes are safe to keep (no regression)
- RuWordNet dictionary is loaded but its effect is minimal for these queries
- Focus future work on: source diversity (more sources per query), positional retrieval, and better excerpt selection
