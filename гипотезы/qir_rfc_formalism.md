# RFC: Quantum Information Retrieval (QIR) Formalism for Tensor Retrieval VM

> Статус: Draft / June 2026
> Цель: формальная основа для quantum-inspired retrieval в APL-π-φ VM
> База: Van Rijsbergen (2004), Zuccon & Azzopardi (2009-2010), Piwowarski & Lalmas (2009), Sordoni et al. (2013), Melucci (2008-2015)

---

## 1. Quantum Probability Ranking Principle (QPRP)

### 1.1. Classical PRP vs Quantum PRP

**Classical PRP** (Robertson, 1977):
Documents are ranked by `P(R|q,d)` — probability of relevance given query and document — with the **independence assumption**: `P(d_i ∧ d_j) = 0` for disjoint events.

**Quantum PRP** (Zuccon & Azzopardi, ECIR/ICTIR 2009-2010):
Removes the independence assumption via quantum interference. The probability of a document being relevant is given by the **Born rule**:

```
P(d|q) = |⟨q|d⟩|²
```

For two documents, the classical additivity fails:

**Classical (Kolmogorovian):**
```
P(A ∨ B) = P(A) + P(B)         (disjoint events)
```

**Quantum (von Neumann):**
```
P(A ∨ B) = P(A) + P(B) + 2√(P(A)P(B))·cos(θ_AB)
```

where `θ_AB` is the phase angle between the two document state vectors. The `cos(θ_AB)` term is the **quantum interference term**.

### 1.2. Cosine Similarity as Interference

This is the key insight: **cosine similarity IS the interference term**.

```
cos(θ_AB) = ⟨d_A|d_B⟩ / (|d_A|·|d_B|)
```

When `θ_AB = 0` (documents identical): `cos(0) = 1` → **constructive interference** → `P(A ∨ B) = 4·P(A)` (super-additive)

When `θ_AB = π/2`: `cos(π/2) = 0` → **no interference** → `P(A ∨ B) = P(A) + P(B)` (classical)

When `θ_AB = π`: `cos(π) = -1` → **destructive interference** → `P(A ∨ B) = P(A) + P(B) - 2√(P(A)P(B))` (sub-additive)

### 1.3. Single-Photon Analogy

The QPRP is derived from the **double-slit experiment**:

```
Position x on detector:  P(x) = |ψ₁(x) + ψ₂(x)|²
                          = |ψ₁(x)|² + |ψ₂(x)|² + 2|ψ₁(x)||ψ₂(x)|cos(δ)
                          = P₁(x) + P₂(x) + Interference
```

Mapping to IR:
- **Slit 1, Slit 2** → Document A, Document B
- **Detector position x** → User's information need
- **Probability P(x)** → Relevance probability
- **δ = phase difference** → Semantic angle between documents

The QPRP ranking criterion:
```
Score(d) = P(d|q) + Σ_{d' > d} Interference(d, d')
```

where the interference captures how document d interacts with already-ranked documents.

### 1.4. References

- **Zuccon, G., Azzopardi, L., van Rijsbergen, C.J.** (2009). "The Quantum Probability Ranking Principle for Information Retrieval." ICTIR 2009, LNCS 5766, pp. 232-240. DOI: 10.1007/978-3-642-12275-0_32
- **Zuccon, G., Azzopardi, L.** (2010). "Using the Quantum Probability Ranking Principle to Rank Interdependent Documents." ECIR 2010, LNCS 5993, pp. 357-369.
- **Zuccon, G.** (2012). "Document Ranking with Quantum Probabilities." PhD Thesis, University of Glasgow.
- **Robertson, S.E.** (1977). "The Probability Ranking Principle in IR." Journal of Documentation, 33(4), 294-304.

### 1.5. APL Mapping

```apl
⍝ Classical cosine (no interference):
scores ← +.× ⍉⍉ query_emb ∘.cos corpus_emb

⍝ QPRP with interference (pairwise document interaction):
phase ← corpus_emb ∘.cos corpus_emb              ⍝ θ_AB matrix
P ← |cos_scores*2                                 ⍝ Born rule probabilities
interference ← 2 × (√P ∘.× √P) × phase            ⍝ 2√(P(A)P(B))cos(θ)
qprp_scores ← +/ interference                      ⍝ per-doc total with interference
ranked ← ⍒ qprp_scores                             ⍝ Quantum ranking
```

**VM OpCode:**
```rust
OpCode::QPRP_RANK { top_k: usize } => {
    let phase_mat = stack.pop_as::<Tensor2D>()?;     // cos(θ) matrix
    let probs = stack.pop_as::<Tensor1D>()?;          // P(d|q) from Born rule
    // Interference: 2 * sqrt(P_i * P_j) * cos(θ_ij)
    let sqrt_p = probs.mapv(|p| p.sqrt());
    let outer = &sqrt_p * &sqrt_p.transpose();        // outer product √P_i · √P_j
    let interference = 2.0 * outer * phase_mat;       // entrywise
    let total: Array1<f32> = interference.sum_axis(Axis(1));
    stack.push(Value::Tensor1D(total));
    // ⍒ for ranking
}
```

---

## 2. Van Rijsbergen's "Geometry of Relevance" (2004)

### 2.1. Hilbert Space Model for IR

The core thesis of **Van Rijsbergen (2004)** — *The Geometry of Information Retrieval* — is that IR models (vector space, probabilistic, logical) can be **unified** within Hilbert space:

| IR Concept | Hilbert Space Analogue |
|---|---|
| Document | State vector `|d⟩ ∈ H` |
| Query | Subspace `S_Q ⊂ H` (or projection operator `P_Q`) |
| Relevance | Probability = squared length of projection |
| Aboutness | `d ⊨ q` iff `|d⟩ ∈ S_Q` |
| Observable | Hermitian operator (e.g., relevance operator `R`) |

### 2.2. Relevance as Projection

Given a query subspace `S_Q` with projection operator `P_Q`:
```
Probability that document d is relevant:
    P(relevant | d, q) = ⟨d|P_Q|d⟩ = ‖P_Q|d⟩‖²
```

If query is a unit vector `|q⟩`: `P_Q = |q⟩⟨q|`

```
P(relevant | d, q) = |⟨q|d⟩|²
```

This is **exactly cosine similarity squared** — the squared magnitude of the projection of the document vector onto the query subspace.

### 2.3. Gleason's Theorem (1957)

The central mathematical result:

> **Gleason's Theorem**: Let H be a separable Hilbert space of dimension ≥ 3. For any probability measure µ on the closed subspaces of H, there exists a unique **density operator** ρ (positive self-adjoint, trace-class with tr(ρ) = 1) such that for all closed subspaces L:
> ```
> µ(L) = tr(ρ P_L)
> ```

**Significance for IR** (Van Rijsbergen, Ch. 4): Any consistent assignment of relevance probabilities to documents (subspaces) corresponds to a density operator. This **justifies** the use of trace-based scoring:
```
Score(d, q) = tr(ρ_q P_d)
```

### 2.4. References

- **Van Rijsbergen, C.J.** (2004). *The Geometry of Information Retrieval*. Cambridge University Press. DOI: 10.1017/CBO9780511543333
- **Gleason, A.M.** (1957). "Measures on the Closed Subspaces of a Hilbert Space." *Journal of Mathematics and Mechanics*, 6, 885-893.
- **Cooke, R., Keane, M., Moran, W.** (1985). "An Elementary Proof of Gleason's Theorem." *Mathematical Proceedings of the Cambridge Philosophical Society*, 98, 117-128.

### 2.5. APL Mapping

```apl
⍝ Query as projection operator P_Q = |q⟩⟨q|
P_Q ← q ∘.× q                                ⍝ outer product: |q⟩⟨q|

⍝ Document vector |d⟩
score ← +/ (+/ P_Q × d) × d                  ⍝ tr(P_Q |d⟩⟨d|) = ⟨d|P_Q|d⟩

⍝ But simpler: Gleason trace formula
Score ← {tr ⍺ +.× ∘.×⍨ ⍵}                   ⍝ ⍺ = ρ_q, ⍵ = |d⟩
```

**VM OpCode:**
```rust
OpCode::GLEASON_SCORE => {
    let density = stack.pop_as::<Tensor2D>()?;   // ρ_q (density matrix for query)
    let doc_vec = stack.pop_as::<Tensor1D>()?;   // |d⟩
    let projector = doc_vec.dot(&doc_vec);        // |d⟩⟨d|
    let score = (density * projector).sum();      // tr(ρ_q · P_d)
    stack.push(Value::Scalar(score));
}
```

---

## 3. Density Matrices for Ambiguous Queries

### 3.1. Pure States vs Mixed States

A **pure state** represents a single, unambiguous information need:
```
|q⟩ → ρ_q = |q⟩⟨q|        (rank-1 projection)
```

A **mixed state** represents uncertainty or multiple facets:
```
ρ = Σ_i p_i |q_i⟩⟨q_i|    where Σ p_i = 1, p_i ≥ 0
```

### 3.2. Multi-faceted Query Representation

For a query with k aspects (e.g., "machine learning" + "healthcare" + "ethics"):
```
ρ_q = p₁|q₁⟩⟨q₁| + p₂|q₂⟩⟨q₂| + p₃|q₃⟩⟨q₃|
```

The probability that document d is relevant to the ambiguous query:
```
P(d|ρ_q) = tr(ρ_q · P_d) = Σ_i p_i · tr(|q_i⟩⟨q_i| · P_d)
         = Σ_i p_i · |⟨q_i|d⟩|²
```

This is a **convex combination** of relevance probabilities — but unlike classical mixture models, the density matrix formalism also captures **interference between query aspects** (off-diagonal terms).

### 3.3. Trace Distance for Comparing Retrieval Results

The trace distance between two density matrices measures how distinguishable two retrieval states are:
```
D(ρ₁, ρ₂) = ½ · tr(|ρ₁ - ρ₂|) = ½ · Σ_i |λ_i|
```
where `λ_i` are eigenvalues of `(ρ₁ - ρ₂)`.

**For retrieval**: If `ρ₁` is the query state and `ρ₂` is the document state, `D(ρ₁, ρ₂)` measures how well the document matches the query.

### 3.4. Von Neumann Entropy for Query Ambiguity

```
S(ρ) = -tr(ρ · log ρ) = -Σ_i λ_i log λ_i
```

Properties:
- Pure state: `S(|q⟩⟨q|) = 0` (no ambiguity)
- Maximally mixed: `S(I/n) = log n` (maximum ambiguity)
- Multi-faceted: `0 < S(ρ) < log n`

This gives a **quantitative measure of query ambiguity** — higher entropy = more ambiguous.

### 3.5. References

- **Sordoni, A., Nie, J.-Y., Bengio, Y.** (2013). "Modeling Term Dependencies with Quantum Language Models for IR." SIGIR 2013, pp. 653-662. DOI: 10.1145/2484028.2484098
- **Sordoni, A., Nie, J.-Y., Bengio, Y.** (2014). "Learning Concept Embeddings for Query Expansion by Quantum Entropy Minimization." AAAI 2014, pp. 1586-1592.
- **Sordoni, A., Nie, J.-Y.** (2014). "Looking at Vector Space and Language Models for IR using Density Matrices." arXiv:1401.1732.
- **Piwowarski, B., Frommholz, I., Lalmas, M., van Rijsbergen, C.J.** (2010). "What can Quantum Theory bring to Information Retrieval?" CIKM 2010, pp. 59-68.
- **Li, Q., Uprety, S., Wang, B., Song, D.** (2018). "Quantum-Inspired Complex Word Embedding." *Proceedings of the Third Workshop on Representation Learning for NLP*, ACL, pp. 52-57.

### 3.6. APL Mapping

```apl
⍝ Density matrix from weighted query aspects
ρ ← +⌿ p ∘.× ∘.×⍨⍉ vectors                    ⍝ Σ p_i |q_i⟩⟨q_i|

⍝ Von Neumann entropy
S ← {-+/ (⍟ λ) × λ} ⊢ eigenvalues ρ            ⍝ -tr(ρ log ρ)

⍝ Trace distance
D ← {0.5 × +/ | eigenvalues (⍺ - ⍵)}           ⍝ ½ · Σ |λ_i|

⍝ Query-document matching via trace
score ← +/ (+/ ρ × P_d)                         ⍝ tr(ρ · P_d)
```

**VM OpCode:**
```rust
OpCode::VON_NEUMANN_ENTROPY => {
    let rho = stack.pop_as::<Tensor2D>()?;      // density matrix
    let (eigvals, _) = eigendecomposition(&rho)?;
    let s = -eigvals.iter()
        .filter(|&&l| l > 1e-10)
        .map(|&l| l * l.ln())
        .sum::<f32>();
    stack.push(Value::Scalar(s));
}

OpCode::TRACE_DISTANCE => {
    let rho2 = stack.pop_as::<Tensor2D>()?;
    let rho1 = stack.pop_as::<Tensor2D>()?;
    let diff = &rho1 - &rho2;
    let (eigvals, _) = eigendecomposition(&diff)?;
    let d = 0.5 * eigvals.iter().map(|l| l.abs()).sum::<f32>();
    stack.push(Value::Scalar(d));
}
```

---

## 4. Quantum Measurement and the Observer Effect

### 4.1. Query as Projective Measurement

The retrieval process is modeled as a **von Neumann measurement**:

```
Before query:  |Φ⟩ = Σ_i α_i |d_i⟩    (corpus in superposition)
Measurement:   {P_Q, I - P_Q}          (query projection + its complement)
After query:   |Φ'⟩ = P_Q|Φ⟩ / ‖P_Q|Φ⟩‖   (collapsed state)
```

### 4.2. Copenhagen Interpretation for Retrieval

The key philosophical stance:

> **"Before the query, the corpus exists in a superposition of all relevance possibilities."**
> The corpus state `|Φ⟩ = Σ_i √(w_i) |d_i⟩` represents all documents weighted by their prior relevance.
> **"The act of querying defines the measurement basis and collapses the superposition to a specific ranking."**
> The query projector `P_Q` selects the basis; the Born rule gives `P(d_i|q) = |⟨q|d_i⟩|²`.
> **"Each measurement perturbs the state."**
> Relevance feedback changes the corpus state — the observer effect in IR.

### 4.3. Sequential Measurements (Progressive Retrieval)

A sequence of measurements with increasingly refined projectors:

```
Measurement 1 (coarse):  P₁ = span{|e₁⟩, ..., |eₖ⟩}       (broad topical)
                 ↓
Partial collapse: |Φ₁⟩ = P₁|Φ⟩ / ‖P₁|Φ⟩‖
                 ↓
Measurement 2 (fine):    P₂ = span{|f₁⟩, ..., |fₘ⟩}       (precise)
                 ↓
Final collapse:  |Φ₂⟩ = P₂|Φ₁⟩ / ‖P₂|Φ₁⟩‖
```

This is exactly the MRL progressive search: coarse dimensions first → partial collapse (broad candidates) → finer dimensions → stronger collapse (precise ranking).

### 4.4. The Observer Effect in IR

Each relevance feedback or query refinement perturbs the corpus state:

```
|Φ_{t+1}⟩ = U_t · P_{Q_t} |Φ_t⟩
```

Where:
- `P_{Q_t}` = projection for query at step t
- `U_t` = user cognitive evolution (unitary transformation)

### 4.5. References

- **Piwowarski, B., Lalmas, M.** (2009). "A Quantum-based Model for Interactive Information Retrieval." ICTIR 2009, LNCS 5766, pp. 224-231.
- **Melucci, M.** (2008). "A Basis for Information Retrieval in Context." *ACM TOIS*, 26(3), 1-41. DOI: 10.1145/1361684.1361685
- **Piwowarski, B., Frommholz, I., Lalmas, M., van Rijsbergen, C.J.** (2010). "What can Quantum Theory bring to Information Retrieval?" CIKM 2010.
- **Frommholz, I., Larsen, B., Piwowarski, B., Lalmas, M., Ingwersen, P., van Rijsbergen, K.** (2010). "A Quantum-based Framework for Interactive Information Retrieval." *IIiX 2010*.

### 4.6. APL Mapping

```apl
⍝ Coarse measurement: project onto first k dimensions
P₁ ← (k ↑ ∘.=⍨ ⍳n)                              ⍝ projection: identity truncation
Φ₁ ← (P₁ +.× Φ) ÷ (| P₁ +.× Φ)                  ⍝ normalized collapse

⍝ Fine measurement: precise query
P₂ ← q ∘.× q                                     ⍝ query projector: |q⟩⟨q|
Φ₂ ← (P₂ +.× Φ₁) ÷ (| P₂ +.× Φ₁)                ⍝ second collapse

⍝ MRL progressive as sequential measurement:
coarse_scores  ← query ∘.cos corpus[; ⍳128]      ⍝ P₁: broad topical
candidates     ← (⍒ coarse_scores) ↑ 100          ⍝ collapse to 100
fine_scores    ← query ∘.cos corpus[candidates;]  ⍝ P₂: precise
results        ← (⍒ fine_scores) ↑ 10             ⍝ final collapse
```

---

## 5. Quantum Interference in Multi-Hop Retrieval

### 5.1. Destructive Interference between Documents

Key phenomenon: **A is relevant, B is relevant, but A+B is not relevant.**

This occurs when:
```
θ_AB ≈ π  →  cos(θ_AB) ≈ -1  →  P(A ∨ B) < P(A) + P(B)
```

Example: Document A about "neural networks", document B about "networks in graph theory". Each is relevant to "network", but together they confuse the topic.

### 5.2. Path Interference in Multi-Step Retrieval

Analogy to the **double-slit experiment**:

```
Path 1: q → doc_A → doc_C    (amplitude: ψ₁ = ⟨doc_C|U|doc_A⟩⟨doc_A|U|q⟩)
Path 2: q → doc_B → doc_C    (amplitude: ψ₂ = ⟨doc_C|U|doc_B⟩⟨doc_B|U|q⟩)

Total:  ψ_C = ψ₁ + ψ₂
P(C) = |ψ₁|² + |ψ₂|² + 2|ψ₁||ψ₂|cos(θ₁₂)
```

**"Which path" information destroys interference:**
If the system tracks which intermediate document was used, the paths become distinguishable and the interference term vanishes. This is analogous to **document independence assumptions** in classical multi-hop — assuming paths are independent removes interference.

### 5.3. Multi-Hop Interference Formula

For a 2-hop path `q → d₁ → d₂`:

```
P(d₂|q) = |⟨d₂|U|d₁⟩⟨d₁|U|q⟩ + Σ_{other paths} ⟨d₂|U|dₖ⟩⟨dₖ|U|q⟩|²
        = |ψ₁|² + |ψ₂|² + 2|ψ₁||ψ₂|cos(δ)
```

The interference term `cos(δ)` captures whether two reasoning paths reinforce or cancel each other.

### 5.4. References

- **Bruza, P., Wang, J., Song, D., Zhang, P., Hou, Y., Zuccon, G., et al.** (2010). "Explanation of Relevance Judgement Discrepancy with Quantum Interference." AAAI Fall Symposium 2010, pp. 117-124.
- **Zhang, P., Song, D., Hou, Y., Wang, J., Bruza, P.** (2010). "Automata Modeling for Cognitive Interference in Users' Relevance Judgment." AAAI Fall Symposium 2010, pp. 125-132.
- **Hou, Y., Song, D., Zhang, P., Wang, J., Bruza, P.** (2010). "Explanation of Relevance Judgement Discrepancy with Quantum Interference." AAAI QI Symposium.

### 5.5. APL Mapping

```apl
⍝ Two-path interference in multi-hop
ψ₁ ← (doc_C ∘.cos doc_A) × (doc_A ∘.cos q)     ⍝ amplitude for path 1
ψ₂ ← (doc_C ∘.cos doc_B) × (doc_B ∘.cos q)     ⍝ amplitude for path 2
θ ← doc_A ∘.cos doc_B                            ⍝ phase between paths

P ← (ψ₁*2) + (ψ₂*2) + 2 × ψ₁ × ψ₂ × θ           ⍝ total probability with interference

⍝ Multi-hop with all path interference:
amplitudes ← (doc_C ∘.cos corpus) × (corpus ∘.cos q)  ⍝ all 2-hop amplitudes
total_amp ← +/ amplitudes                              ⍝ coherent sum
P_C ← |total_amp*2                                     ⍝ Born rule
```

---

## 6. Tensor Product Spaces for Multi-Modal Retrieval

### 6.1. Composite Systems

Multi-modal retrieval (text + image + graph) is modeled as a **tensor product space**:

```
H_total = H_text ⊗ H_image ⊗ H_graph
```

A multi-modal document is represented as a **product state** (if independent):

```
|d⟩ = |d_text⟩ ⊗ |d_image⟩ ⊗ |d_graph⟩
```

### 6.2. Entangled Document Pairs

Two documents may exhibit **quantum entanglement** — correlations that cannot be factored:

```
|ψ⟩ = α|d₁⟩|d₂⟩ + β|d₂⟩|d₁⟩
```

Meaning: "document 1 is about topic A and document 2 about topic B, OR vice versa." This captures **citation relationships**, **complementary information**, or **paraphrase pairs**.

### 6.3. Schmidt Decomposition

Any bipartite state can be decomposed:

```
|ψ⟩ = Σ_i σ_i |a_i⟩ ⊗ |b_i⟩
```

where `σ_i ≥ 0` (Schmidt coefficients), `Σ σ_i² = 1`, and `{|a_i⟩}, {|b_i⟩}` are orthonormal.

- **Schmidt rank** = number of non-zero σ_i: measures degree of correlation
- **Schmidt rank = 1**: product state (no correlation, documents independent)
- **Schmidt rank > 1**: entanglement (non-classical correlation)

### 6.4. Bell Inequality Violation for Semantic Entanglement

The CHSH inequality can detect non-classical semantic correlations:

```
S = |E(a,b) - E(a,b')| + |E(a',b) + E(a',b')|
```

- Classical bound: `S ≤ 2`
- Quantum bound: `S ≤ 2√2 ≈ 2.828` (Tsirelson bound)
- Violation → "entangled" semantic concepts

### 6.5. References

- **Piwowarski, B., Lalmas, M.** (2009). "A Quantum-based Model for Interactive Information Retrieval." ICTIR 2009. (Sections on tensor product IN spaces)
- **Melucci, M.** (2010). "Deriving a Quantum Information Retrieval Basis." *The Computer Journal*, 56(11), 1279-1291.
- **Widdows, D., Bruza, P., Peters, S., Cavedon, L.** (2000-2009). Series of papers on quantum logic for NLP (orthogonal negation, tensor products).
- Quantum Interaction Symposia proceedings: QI 2007-2015 (AAAI/CEUR).
- **Li, Q., et al.** (2019). "Complex-valued Network for Matching (CNM)."

### 6.6. APL Mapping

```apl
⍝ Tensor product (Kronecker product) for multi-modal
d_total ← text_emb ∘.× image_emb                 ⍝ |d_text⟩ ⊗ |d_image⟩

⍝ Schmidt decomposition via SVD
σ ← singular_values matrix                        ⍝ Schmidt coefficients
schmidt_rank ← +/ σ > ε                           ⍝ correlation degree

⍝ Entanglement test: if S > 2, non-classical
S ← CHSH_test query_docs                          ⍝ Bell inequality
```

---

## 7. Mathematical Toolbox

### 7.1. Born Rule

```
P(relevant|q, d) = |⟨q|d⟩|²
```

This is the fundamental probability rule in QIR. In terms of cosine:
```
P = cos²(θ_qd)
```

### 7.2. Fidelity

Measures similarity between two quantum states (density matrices):
```
F(ρ₁, ρ₂) = tr(√(√ρ₁ · ρ₂ · √ρ₁))
```

For pure states `|ψ⟩, |φ⟩`: `F = |⟨ψ|φ⟩|`
For classical probability distributions: reduces to Bhattacharyya distance.

**In IR**: measures how similar two query representations (or document representations) are, accounting for ambiguity.

### 7.3. Quantum Relative Entropy

```
S(ρ₁‖ρ₂) = tr(ρ₁(log ρ₁ - log ρ₂))
```

Generalization of KL-divergence:
- Classical KL-divergence: `KL(P‖Q) = Σ P(i) log(P(i)/Q(i))`
- For diagonal density matrices: QRE = KL-divergence
- For non-diagonal: captures quantum correlations that KL misses

**In IR**: measures information gain when moving from prior (ρ₂) to posterior (ρ₁) query state.

### 7.4. Positive Operator-Valued Measures (POVM)

Generalization of projective measurements for **graded relevance**:

A POVM is a set of positive operators `{E_m}` with `Σ_m E_m = I`:
```
P(outcome = m) = tr(ρ · E_m)
```

For graded relevance (e.g., 1-5 scale), define `E_1, ..., E_5` where each `E_m` corresponds to a relevance grade. The probability of grade m for document d:
```
P(grade = m | d) = tr(|d⟩⟨d| · E_m)
```

### 7.5. Kraus Operators for Retrieval Transformations

Any linear, completely positive, trace-preserving map (quantum channel) can be written:
```
Φ(ρ) = Σ_i K_i ρ K_i†    where Σ_i K_i†K_i = I
```

**Applications in IR pipeline:**
| Transformation | Kraus Operators | Description |
|---|---|---|
| Embedding | `K_1 = W_embed` | Linear projection to embedding space |
| Summarization | `K_i = |summary_i⟩⟨context_i|` | Query→summary channel |
| Reranking | `K_i = √(w_i) · P_i` | Weighted projection onto relevance grades |
| Filtering | `K_0 = P_keep, K_1 = P_discard` | Selection channel |
| Query expansion | `K_t = |t⟩⟨q|` for each expansion term t | Term addition |

### 7.6. References

- **Sordoni, A., Nie, J.-Y.** (2014). "Looking at Vector Space and Language Models for IR using Density Matrices." arXiv:1401.1732. [Shows KL-divergence as special case of VN-divergence]
- **Li, Q., Uprety, S., Wang, B., Song, D.** (2018). "Quantum-Inspired Complex Word Embedding." ACL Workshop on Representation Learning for NLP.
- **Zhang, P., Niu, J., Su, Z., Wang, B., Ma, L., Song, D.** (2018). "End-to-End Quantum-like Language Models with Application to Question Answering." AAAI 2018, 32(1). DOI: 10.1609/aaai.v32i1.11979
- **Melucci, M.** (2015). *Introduction to Information Retrieval and Quantum Mechanics*. Springer. [Most comprehensive textbook on QIR]

### 7.7. APL Mapping

```apl
⍝ Born rule: P = |⟨q|d⟩|²
P ← (q ∘.cos d)*2

⍝ Fidelity for density matrices
F ← {+/ | eigenvalues (√⍺) +.× (√⍵)}            ⍝ tr(√(√ρ₁·ρ₂·√ρ₁))

⍝ Quantum relative entropy (VN divergence)
S_qre ← {+/ (+/ ⍺ × ⍟ ⍺) - (+/ ⍺ × ⍟ ⍵)}       ⍝ tr(ρ(log ρ - log σ))

⍝ Graded relevance via POVM
grade_probs ← { |tr ⌈.× (⍵ ∘.× ⍉⍵) +.× E_m}    ⍝ P(grade = m | d)

⍝ Kraus channel: reranking as weighted projections
rerank ← {sum K_i +.× ρ +.× ⍉ K_i}               ⍝ Σ K_i ρ K_i†
```

---

## 8. Practical QIR Systems

### 8.1. Quantum Language Model (QLM)

**Sordoni, Nie, Bengio (SIGIR 2013)**: The first practical QIR system.

- Represents words and sentences as **density matrices** in a semantic Hilbert space
- Uses **Von Neumann divergence** as the matching score
- Captures **term dependencies** via off-diagonal elements of density matrices

Architecture:
```
Word → |w⟩ (state vector) → density matrix ρ = Σ P(w) |w⟩⟨w|
Sentence → ρ_sentence = Σ_i ρ_word_i (mixture)
Query-document match: VN(ρ_q‖ρ_d) = tr(ρ_q(log ρ_q - log ρ_d))
```

### 8.2. Quantum-inspired BM25

BM25 can be re-expressed as an interference-based scoring function:

```
BM25(q, d) = Σ_{t ∈ q∩d} w_t · IDF(t) · BM25_score(t, d)
```

Interference interpretation (Melucci, 2012):
```
BM25(q, d) = Σ_t P(t|d) 
              + Σ_{t≠s} Interference(t,s,d)   [implicit via saturation]
```

### 8.3. Neural Quantum Language Models

**Zhang et al. (AAAI 2018)**: NNQLM — Neural Network Quantum-like Language Model.

- Word embeddings → complex-valued vectors in Hilbert space
- Sentence: density matrix from word vectors
- Matching: 2D-CNN over density matrix features
- State-of-the-art on TREC-QA, WIKIQA

### 8.4. Quantum Expectation Value LM (QEV-LM)

**Zhao et al. (Entropy 2020)**: A shared density matrix for the entire semantic space.

- Words/sentences = **observables** (Hermitian operators), not states
- Matching score = quantum expectation value: `⟨ψ_shared|O_qa|ψ_shared⟩`
- Faster than per-sentence density matrix construction

### 8.5. SIGIR Forum Surveys and Special Issues

- **Li, Y., et al.** (2008). "Geometric and Quantum Methods for Information Retrieval." *ACM SIGIR Forum*, 42(2), 22-32.
- **Song, D., Lalmas, M., van Rijsbergen, K., et al.** (2010). "How Quantum Theory is Developing the Field of Information Retrieval." AAAI Fall Symposium 2010.
- **Melucci, M., van Rijsbergen, C.J.** (2012). "Quantum Mechanics and Information Retrieval." *SIGIR Forum* Special Section.
- **Melucci, M.** (2015). *Introduction to Information Retrieval and Quantum Mechanics*. The Information Retrieval Series, Vol. 35, Springer.

### 8.6. APL Mapping

```apl
⍝ QLM matching via VN divergence
VN ← {+/ (+/ ⍺ × ⍟ ⍺) - +/ (+/ ⍺ × ⍟ ⍵)}       ⍝ tr(ρ_q(log ρ_q - log ρ_d))

⍝ Quantum expectation value matching
O_qa ← q_obs +.× a_obs                           ⍝ joint observable
score ← +/ (+/ ρ_shared × O_qa)                   ⍝ ⟨ψ|O|ψ⟩ = tr(ρ·O)

⍝ Quantum-inspired BM25 with term interference
bm25_qir ← { +/ (tf × idf) + 2 × inter_term_matrix × √(tf∘.×idf) }
```

---

## 9. Collapse as Progressive Retrieval

### 9.1. Weak-to-Strong Measurement Sequence

The retrieval process is a **sequence of quantum measurements** with increasing measurement strength:

```
Step 1 (weak/broad):   Projector P₁ with tr(P₁) = k₁     (many dims, coarse)
                        → candidates: C₁ = Top-k₁(P₁|Φ⟩)

Step 2 (medium):        Projector P₂ ⊂ P₁ with tr(P₂) = k₂ (fewer dims, finer)
                        → candidates: C₂ = Top-k₂(P₂|Φ₁⟩)

Step 3 (strong/precise): Projector P₃ ⊂ P₂                 (fine dimensions)
                         → results: C₃ = Top-k₃(P₃|Φ₂⟩)
```

Each step induces a **partial collapse** — the corpus state reduces to a smaller subspace.

### 9.2. The Observer Effect in Progressive Retrieval

Critical insight: **each measurement perturbs the set of candidates**.

After first measurement (coarse): the corpus is in state `|Φ₁⟩ = P₁|Φ⟩ / ‖P₁|Φ⟩‖`.
- Documents with high probability in the coarse subspace survive
- But some documents relevant in fine dimensions may have been filtered out
- This is the **quantum Zeno effect**: too coarse a measurement "freezes" the state

### 9.3. Optimal Measurement Schedule

The sequence `(P₁, P₂, P₃)` with dimensions `(d₁, d₂, d₃)` defines the progressive schedule. For MRL models:

```
d_sequence = [128, 256, 512, 1024]
k_sequence = [1000, 200, 50, 10]
```

Each progressive step is a **von Neumann measurement** with projector `P_d = span{e_1,...,e_d}`.

### 9.4. References

- **Melucci, M.** (2012). "Contextual Search: A Computational Framework." *Foundations and Trends in IR*, 6, 257-405.
- **Melucci, M.** (2008). "A Basis for Information Retrieval in Context." *ACM TOIS*, 26(3). [Sequential measurement model]
- **Piwowarski, B., Lalmas, M.** (2009). "A Quantum-based Model for Interactive Information Retrieval." ICTIR 2009. [Interaction as sequential measurement]

### 9.5. APL Mapping

```apl
⍝ Progressive retrieval as measurement sequence
MRL_PROGRESSIVE ← {
    dims ← 128 512 1024                             ⍝ measurement schedule
    ks   ← 100 50 10                                ⍝ collapse thresholds
    corpus ← ⍵                                      ⍝ initial corpus
    query  ← ⍺                                      ⍝ query vector

    candidates ← ⍳≢corpus                           ⍝ all documents
    :For d k :InEach dims ks
        scores  ← query ∘.cos corpus[candidates; ⍳d]
        order   ← ⍒ scores                          ⍝ grade down
        candidates ← candidates[order ↑ k]           ⍝ collapse
    :EndFor
    candidates
}
```

---

## 10. APL-π-φ Framework Connection

### 10.1. π as the Natural Phase Angle

In quantum phase space, `π` appears as the fundamental phase:
- Constructive interference: `θ = 0` → `cos(0) = 1`
- Destructive interference: `θ = π` → `cos(π) = -1`
- Orthogonal: `θ = π/2` → `cos(π/2) = 0`

**In our VM**: `π` is the natural scale for phase angles in `TENSOR_FOURIER` (Random Fourier Features) and in interference calculations.

```apl
⍝ Phase angle normalization
θ ← π × (corpus ∘.cos corpus)                      ⍝ scale cosine to [0, π]

⍝ Quantum Fourier features (Rahimi & Recht, 2007)
φ ← cos(ω +.× x × π)                               ⍝ phase features
```

### 10.2. φ as the Golden Ratio in Squeezed States

The golden ratio `φ = (1+√5)/2 ≈ 1.618` appears in quantum optics through:
- **Squeezed states**: minimum uncertainty states with quadrature variance ratio = φ
- **Optimal phase estimation**: φ appears in the Fisher information bound
- **Fibonacci anyons**: topological quantum computing

**In our VM**: `φ` could parameterize optimal squeezing for dimension reduction:

```apl
⍝ Squeezing operator parameterized by φ
squeeze_factor ← φ * 0.5                             ⍝ optimal squeezing
squeezed_dims ← corpus × (squeeze_factor * normal_distribution)
```

### 10.3. APL Operators as Measurement Operators

| APL Symbol | Quantum Interpretation | IR Operation |
|---|---|---|
| `+.×` | Inner product / Born probability | `⟨q|d⟩` → relevance probability |
| `⍒` (grade down) | **Collapse operator** | Ranking = collapsing superposition to ordered list |
| `⍸` (where) | Measurement outcome filter | Select relevant dimension indices |
| `∘.f` (outer) | Tensor product | Multi-modal composition |
| `↑` (take) | Subspace projection | MRL dimension slice |
| `∇` (self-reference) | Recursive measurement | Multi-hop iterative collapse |
| `⍉` (transpose) | Basis change | Query reformulation / rotation |
| `f/` (reduce) | Partial trace | Marginalization over dimensions |
| `φ` (FFT) | Phase space transformation | Random Fourier Features |
| `⌹` (matrix divide) | Pseudo-inverse / inference | Quantum state tomography |

### 10.4. MRL_PROGRESSIVE_SEARCH as Measurement Sequence

The existing `MRL_PROGRESSIVE_SEARCH` OpCode (from RFC) maps directly to **sequential projective measurement**:

```
OpCode::MRL_PROGRESSIVE_SEARCH { dims, top_k }
    = Sequence of projectors P_d = span{e_1, ..., e_d}
    = Weak measurement (P_128) → Strong measurement (P_1024)
    = Progressive collapse of corpus state
```

### 10.5. Complete QIR-to-APL Mapping Table

| QIR Concept | Formal Definition | APL Translation | VM OpCode |
|---|---|---|---|
| State vector | `|d⟩ ∈ H` | `d` (Tensor1D) | LoadConst |
| Density matrix | `ρ = Σ p_i |i⟩⟨i|` | `p ∘.× ∘.×⍨⍉ vectors` | DENSITY_MATRIX |
| Born rule | `P(q|d) = |⟨q|d⟩|²` | `(q +.× d)*2` | BORN_PROBABILITY |
| Projective meas. | `P_Q = |q⟩⟨q|` | `q ∘.× q` | PROJECTION |
| Interference | `2√(P₁P₂)cosθ` | `2 × √(P∘.×P) × cosΘ` | QPRP_INTERFERENCE |
| Collapse | `P_Q|Φ⟩ → ranked` | `(⍒ scores) ↑ k` | TENSOR_GRADE_DOWN |
| VN entropy | `-tr(ρ log ρ)` | `-+/ (⍟ λ) × λ` | VON_NEUMANN_ENTROPY |
| Trace distance | `½tr|ρ₁-ρ₂|` | `0.5×+/|λ` | TRACE_DISTANCE |
| Fidelity | `tr√(√ρ₁ρ₂√ρ₁)` | `F(ρ₁; ρ₂)` | FIDELITY |
| POVM | `{E_m}, ΣE_m=I` | `grade_probs` | POVM_MEASURE |
| Kraus channel | `Φ(ρ)=ΣK_iρK_i†` | `sum K_i +.× ρ +.× ⍉K_i` | KRAUS_CHANNEL |
| Tensor product | `H₁⊗H₂` | `∘.×` | TENSOR_OUTER_PRODUCT |
| Schmidt decomp. | `|ψ⟩=Σσ_i|a_i⟩|b_i⟩` | SVD | SINGULAR_VALUES |
| Sequential meas. | `P_₁ → P_₂ → ...` | MRL_PROGRESSIVE | MRL_PROGRESSIVE_SEARCH |

---

## Appendix A: Reference Timeline

| Year | Author(s) | Contribution | Venue |
|---|---|---|---|
| 1957 | Gleason | Gleason's Theorem | J. Math. Mech. |
| 1977 | Robertson | Probability Ranking Principle | J. Documentation |
| 2004 | van Rijsbergen | *Geometry of IR* (Hilbert space unification) | CUP Book |
| 2007 | Widdows, Bruza | Quantum logic for NLP | QI/AAAI |
| 2008 | Melucci | Contextual IR basis | ACM TOIS |
| 2008 | Li et al. | QIR survey | SIGIR Forum |
| 2009 | Zuccon, Azzopardi, van Rijsbergen | QPRP (first formulation) | ICTIR |
| 2009 | Piwowarski, Lalmas | Interactive QIR model | ICTIR |
| 2010 | Zuccon, Azzopardi | Empirical QPRP evaluation | ECIR |
| 2010 | Piwowarski et al. | QIR framework with tensor products | CIKM |
| 2010 | Song, Lalmas et al. | "How QT is developing IR" | AAAI FS |
| 2011 | Zuccon, Piwowarski, Azzopardi | Complex numbers in QIR | ICTIR |
| 2012 | Melucci | *Contextual Search* monograph | Found. & Trends |
| 2013 | Sordoni, Nie, Bengio | QLM (first practical system) | SIGIR |
| 2013 | Melucci | Deriving QIR basis | Comp. Journal |
| 2014 | Sordoni, Bengio, Nie | QE via quantum entropy | AAAI |
| 2014 | Sordoni, Nie | Density matrices for LM/VSM | arXiv |
| 2015 | Melucci | *Intro to IR and QM* (textbook) | Springer |
| 2018 | Zhang et al. | NNQLM | AAAI |
| 2018 | Li et al. | Complex-valued word embeddings | ACL Workshop |
| 2019 | Li et al. | CNM (Complex-valued Network for Matching) | ACL |
| 2020 | Li et al. | Entanglement embeddings for QLM | AAAI |
| 2020 | Zhao et al. | QEV-LM (expectation value) | Entropy |
| 2024 | — | Bell-like testing for document ranking | Entropy |

## Appendix B: Key Theorems for RFC

### B.1. Gleason's Theorem (formal)

> Let µ be any probability measure on the closed subspaces of a separable (real or complex) Hilbert space H of dimension ≥ 3. There exists a unique positive self-adjoint operator D of trace-class such that, for all closed subspaces L of H:
> ```
> µ(L) = tr(D · P_L)
> ```
> where P_L is the projection operator onto L.
>
> If µ is a probability measure (µ(H) = 1), then tr(D) = 1, so D is a **density operator**.

**IR implication**: Any consistent system for assigning relevance probabilities to documents (subspaces) corresponds to a density matrix. The trace formula `tr(ρ · P_d)` is the **only possible** way to compute relevance probabilities.

### B.2. Quantum Probability vs Classical Probability

| Property | Classical (Kolmogorov) | Quantum (von Neumann) |
|---|---|---|
| Events | Subsets of a sample space Ω | Subspaces of Hilbert space H |
| Probability | `P : 2^Ω → [0,1]` | `P : Subspaces(H) → [0,1]` |
| Additivity | `P(A∪B) = P(A)+P(B)` for disjoint A,B | `P(A∨B) = P(A)+P(B)+Interference` |
| Distributivity | `A∧(B∨C) = (A∧B)∨(A∧C)` | NOT distributive — this is key! |
| Logic | Boolean | Quantum logic (orthomodular lattice) |
| Composite | Product measure | Tensor product |

### B.3. Kraus Representation Theorem

> A linear map Φ : B(H₁) → B(H₂) is a **quantum channel** (completely positive and trace-preserving) if and only if it admits a representation:
> ```
> Φ(ρ) = Σ_i K_i ρ K_i†
> ```
> where K_i : H₁ → H₂ are linear operators (Kraus operators) satisfying Σ_i K_i†K_i = I.

---

*This document is intended as mathematical background for the APL-π-φ tensor retrieval VM RFC. All equations, operators, and mappings are designed for direct translation into VM OpCodes and APL surface syntax.*
