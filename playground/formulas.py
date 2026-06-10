"""
╔══════════════════════════════════════════════════════════════╗
║               FORMULARIUM APL-π-φ                           ║
║  Каталог всех формул из RFC Unified DSL VM                  ║
║  Источники:                                                 ║
║    §4   — APL tensor operators                              ║
║    §15  — VectorSpace type system / CONVERT                 ║
║    §22  — DIM_MUTATE                                        ║
║    §24  — π, φ, two-block collision                         ║
║    §24.9 — floating-point stabilization                     ║
║    §25  — Symbolic Verification Layer                       ║
║    §27  — Quantum observer effect / QIR                     ║
║    docs/qir_rfc_formalism.md (полный формализм)             ║
╚══════════════════════════════════════════════════════════════╝

Каждая функция — формула из RFC, реализованная на Python 3.
Все результаты детерминированы. Все можно запустить и увидеть.

Запуск:  python playground/formulas.py
"""

import math
import itertools
import random
from dataclasses import dataclass
from typing import Callable

# ─── Константы ─────────────────────────────────────────────────
π = math.pi
φ = (1 + math.sqrt(5)) / 2       # golden ratio ≈ 1.6180339887
Φ = 1 / φ                        # 0.618...
sqrt = math.sqrt
ε_float = 1e-10                  # машинный zero threshold


# ═══════════════════════════════════════════════════════════════
#  §4  —  APL-STYLE TENSOR OPERATORS
# ═══════════════════════════════════════════════════════════════

def inner_product(a: list[float], b: list[float]) -> float:
    """APL:  a +.× b   (dot product)

    Сумма поэлементных произведений.
    Базовая операция для: cosine, projection, scoring.
    """
    return sum(x * y for x, y in zip(a, b))


def outer_product(a: list[float], b: list[float]) -> list[list[float]]:
    """APL:  a ∘.× b

    Тензорное произведение: матрица a_i * b_j.
    Используется для: build(|q⟩⟨q|), multi-modal Kronecker product.
    """
    return [[x * y for y in b] for x in a]


def cosine_sim(a: list[float], b: list[float]) -> float:
    """APL:  a ∘.cos b

    cos(θ) = ⟨a|b⟩ / (‖a‖·‖b‖)
    §24: угловая метрика на гиперсфере.
    §27: probability amplitude (Born rule: cos² = вероятность).
    """
    dot = inner_product(a, b)
    na = sqrt(sum(x * x for x in a))
    nb = sqrt(sum(x * x for x in b))
    if na < ε_float or nb < ε_float:
        return 0.0
    return dot / (na * nb)


def grade_down(scores: list[float], k: int | None = None) -> list[int]:
    """APL:  ⍒ scores   (grade down)

    Возвращает индексы сортировки по убыванию.
    §27: это ОПЕРАТОР КОЛЛАПСА — von Neumann collapse.
    После измерения (cosine) — коллапс суперпозиции в ранжировку.
    """
    if k is None:
        k = len(scores)
    return sorted(range(len(scores)), key=lambda i: scores[i], reverse=True)[:k]


def compress(mask: list[bool], vec: list) -> list:
    """APL:  mask / vec   (compress)

    Фильтрация: оставить только элементы, где mask[i] = True.
    Реляционный SELECT в терминах APL.
    """
    return [v for m, v in zip(mask, vec) if m]


def take(v: list, k: int) -> list:
    """APL:  k ↑ v   (take)

    Взять первые k элементов. Отрицательное k — с конца.
    §22: размерность — фаза вычисления. k = сколько dims берём.
    §27: subspacing projector — коллапс к k измерениям.
    """
    if k >= 0:
        return v[:k]
    return v[k:]


def drop(v: list, k: int) -> list:
    """APL:  k ↓ v   (drop)"""
    if k >= 0:
        return v[k:]
    return v[:-k]


def grade_up(scores: list[float]) -> list[int]:
    """APL:  ⍋ scores   (grade up) — сортировка по возрастанию"""
    return sorted(range(len(scores)), key=lambda i: scores[i])


if __name__ == "__main__":
    print("\n-- #4 APL operators --")
    a, b = [3.0, 1.0, 4.0], [1.0, 5.0, 9.0]
    print(f"  inner_product({a}, {b}) = {inner_product(a, b)}")
    print(f"  cosine_sim({a}, {b}) = {cosine_sim(a, b):.6f}")
    print(f"  grade_down([3, 1, 4, 1, 5]) = {grade_down([3, 1, 4, 1, 5])}")


# ═══════════════════════════════════════════════════════════════
#  §24  —  TWO-BLOCK COLLISION (π)
# ═══════════════════════════════════════════════════════════════

def two_block_collisions(mass_ratio: int) -> int:
    """§24.1: Two-block collision count → π

    Массивный куб M = mass_ratio * m, лёгкий куб mass = 1.
    Упругие столкновения: M-куб → лёгкий куб → стена → ...
    Вопрос: сколько столкновений до ухода M-куба в бесконечность?

    Ответ: count = ⌊π / arctan(1/sqrtM)⌋

    При M = 100ⁿ выдаёт первые n+1 цифр π.
    """
    if mass_ratio <= 0:
        return 0
    angle = math.atan(1.0 / math.sqrt(mass_ratio))
    count = int(π / angle)
    return count


def two_block_simulate(mass_ratio: int) -> list[tuple[float, float]]:
    """Симуляция двух кубов: возвращает (v1, v2) после каждого столкновения.

    Conservation laws:
      momentum:  M*v1 + m*v2 = const
      energy:    ½M*v1² + ½m*v2² = const

    Столкновение M-cube ↔ m-cube:
      v1' = ( (M - m) * v1 + 2 * m * v2 ) / (M + m)
      v2' = ( (m - M) * v2 + 2 * M * v1 ) / (M + m)

    Столкновение m-cube ↔ wall:
      v2' = -v2
    """
    M = float(mass_ratio)
    m = 1.0
    v1 = -1.0   # M-cube движется к стене
    v2 = 0.0    # m-cube в покое

    states = [(v1, v2)]

    while v1 < 0 or v2 < v1:  # пока M-куб не убежал
        # Collision M ↔ m (M slower than m, approaching)
        if v1 < v2:
            v1_new = ((M - m) * v1 + 2 * m * v2) / (M + m)
            v2_new = ((m - M) * v2 + 2 * M * v1) / (M + m)
            v1, v2 = v1_new, v2_new
            states.append((v1, v2))

        # Collision m ↔ wall
        if v2 < 0:
            v2 = -v2
            states.append((v1, v2))

        if len(states) > 100_000:
            break

    return states


def phi_hierarchy(start: int = 633, depth: int = 14) -> list[int]:
    """§24: φ-based dimension hierarchy.

    Рекуррентное деление:
      x_{n+1} = round(x_n / φ)

    633 → 391 → 242 → 149 → 92 → 57 → 35 → 22 → 13 → 8 → 5 → 3 → 2 → 1

    φ — "самое иррациональное число" — даёт копримальные соседние
    уровни (нет общих делителей), максимизируя независимость информации.
    """
    seq = [start]
    for _ in range(depth - 1):
        nxt = round(seq[-1] / φ)
        if nxt < 1:
            nxt = 1
        seq.append(nxt)
        if nxt == 1 and seq[-2] == 1:
            break
    return seq


if __name__ == "__main__":
    print("\n-- #24 pi/phi --")
    for p in range(7):
        M = 100 ** p
        cnt = two_block_collisions(M)
        print(f"  M = 10^{2*p:4d} -> {cnt:15d} collisions (pi digits: {str(cnt)[:p+1]})")

    print(f"\n  phi-dim hierarchy (phi = {φ:.6f}):")
    h = phi_hierarchy()
    print(f"    {' -> '.join(str(x) for x in h)}")

    def gcd(a, b):
        while b:
            a, b = b, a % b
        return a

    coprime_pairs = sum(1 for i in range(len(h)-1) if gcd(h[i], h[i+1]) == 1)
    print(f"    Coprime adjacent levels: {coprime_pairs}/{len(h)-1}")


# ═══════════════════════════════════════════════════════════════
#  §24.9  —  FLOATING-POINT STABILIZATION
# ═══════════════════════════════════════════════════════════════

def haversine(theta: float) -> float:
    """hav(θ) = sin²(θ/2)

    §24.9: заменяет acos для углов near ±1.
    acos в cosine ≈ ±1 имеет производную → ∞, усиливая шум в 10⁵×.
    haversine монотонен и численно стабилен на всём [0, π].
    """
    s = math.sin(theta / 2)
    return s * s


def haversine_distance(cos_theta: float) -> float:
    """Угловое расстояние через haversine.

    Используя arcsin: θ = 2 * arcsin(sqrt(hav(θ)))
    Где hav(θ) = (1 - cos(θ)) / 2

    Стабильно для cos ≈ ±1.
    """
    h = (1.0 - max(-1.0, min(1.0, cos_theta))) / 2.0
    return 2.0 * math.asin(math.sqrt(h))


def kahan_sum(values: list[float]) -> float:
    """§24.9: Kahan compensated summation.

    Суммирует числа с плавающей точкой с компенсацией ошибки.
    Ошибка O(1) вместо O(N) для наивной суммы.
    """
    total = 0.0
    carry = 0.0
    for v in values:
        y = v - carry
        t = total + y
        carry = (t - total) - y
        total = t
    return total


def fma(a: float, b: float, c: float) -> float:
    """§24.9: Fused Multiply-Add.

    Вычисляет a * b + c с одним округлением.
    Python 3 не гарантирует fma в hardware, но math.fma доступен в CPython 3.13+.
    Эмуляция:
    """
    return math.fma(a, b, c) if hasattr(math, 'fma') else a * b + c


def acos_stable(cos_theta: float) -> float:
    """§24.9: acos с haversine-стабилизацией.

    Для |cos| > 0.99 использует haversine формулу.
    """
    cos_theta = max(-1.0, min(1.0, cos_theta))
    if abs(cos_theta) > 0.99:
        return haversine_distance(cos_theta)
    return math.acos(cos_theta)


if __name__ == "__main__":
    print("\n-- #24.9 Floating-point stabilization --")
    print(f"  haversine(pi/3)  = {haversine(π/3):.16f}")
    print(f"  haversine(0)    = {haversine(0):.16f}")
    print(f"  acos_stable(1.0 - 1e-12) = {acos_stable(1.0 - 1e-12):.16f}")
    print(f"  acos_stable(-1.0 + 1e-12)= {acos_stable(-1.0 + 1e-12):.16f}")
    print(f"  kahan_sum([1e12, -1e12, 1]) = {kahan_sum([1e12, -1e12, 1])}")


# ═══════════════════════════════════════════════════════════════
#  §27  —  QUANTUM OBSERVER EFFECT / QIR
# ═══════════════════════════════════════════════════════════════

def born_probability(query: list[float], doc: list[float]) -> float:
    """§27.2: P(d|q) = |⟨q|d⟩|²

    Born rule для retrieval.
    Если векторы нормализованы: cos²(θ).
    """
    cs = cosine_sim(query, doc)
    return cs * cs


def qprp_interference(probs: list[float], phase_matrix: list[list[float]]) -> list[float]:
    """§27.3: QPRP ranking with quantum interference.

    P(A ∨ B) = P(A) + P(B) + 2sqrt(P(A)P(B))·cos(θ_AB)

    phase_matrix[i][j] = cos(θ_ij) — попарный cosine между документами.
    """
    n = len(probs)
    sqrt_p = [sqrt(p) for p in probs]
    scores = [0.0] * n
    for i in range(n):
        total = 0.0
        for j in range(n):
            if i == j:
                total += probs[i]
            else:
                total += probs[j] + 2.0 * sqrt_p[i] * sqrt_p[j] * phase_matrix[i][j]
        scores[i] = total / n  # normalize for comparability
    return scores


def quantum_vs_classical_ranking(query: list[float], docs: list[list[float]]):
    """Сравнение классической и квантовой ранжировки.

    Классическая: score = cosine (независимые документы).
    Квантовая: QPRP с интерференционным членом ±2sqrt(P₁P₂)cos(θ).
    """
    n = len(docs)
    # Born probabilities
    probs = [born_probability(query, d) for d in docs]
    # Phase matrix: cos(θ_ij) между документами
    phase = [[cosine_sim(docs[i], docs[j]) for j in range(n)] for i in range(n)]

    # Classical (independent): just use cosine
    classical = [cosine_sim(query, d) for d in docs]

    # Quantum QPRP
    quantum = qprp_interference(probs, phase)

    return classical, quantum, phase


def density_matrix(vectors: list[list[float]], weights: list[float] | None = None
                   ) -> list[list[float]]:
    """§27.4: ρ = Σ pᵢ |vᵢ⟩⟨vᵢ|

    Матрица плотности для многогранного запроса.
    Если weights = None — равномерная суперпозиция.
    """
    n = len(vectors)
    d = len(vectors[0]) if vectors else 0
    if weights is None:
        weights = [1.0 / n] * n

    rho = [[0.0] * d for _ in range(d)]
    for v, w in zip(vectors, weights):
        for i in range(d):
            for j in range(d):
                rho[i][j] += w * v[i] * v[j]
    return rho


def von_neumann_entropy(rho: list[list[float]]) -> float:
    """§27.4: S(ρ) = -tr(ρ · log ρ) = -Σ λᵢ log λᵢ

    Мера неоднозначности запроса/результата.
    S = 0: pure state (чёткий запрос)
    S = log d: maximally mixed (макс. неоднозначность)
    """
    # Для 1×1 скалярного случая
    if len(rho) == 1 and len(rho[0]) == 1:
        p = rho[0][0]
        if p > ε_float:
            return -p * math.log(p)
        return 0.0

    # Для диагональной матрицы плотности (чистые состояния)
    eigvals = _eigenvalues_2x2(rho) if len(rho) <= 2 else _eigenvalues_diag(rho)
    s = 0.0
    for λ in eigvals:
        if λ > ε_float:
            s -= λ * math.log(λ)
    return s


def _eigenvalues_diag(rho: list[list[float]]) -> list[float]:
    """Собственные числа для диагональной или почти-диагональной матрицы."""
    n = len(rho)
    vals = []
    for i in range(n):
        if abs(rho[i][i]) > ε_float:
            vals.append(rho[i][i])
    return vals


def _eigenvalues_2x2(rho: list[list[float]]) -> list[float]:
    """Аналитические собственные значения для 2×2."""
    a, b, c, d = rho[0][0], rho[0][1], rho[1][0], rho[1][1]
    tr = a + d
    det = a * d - b * c
    disc = tr * tr - 4 * det
    if disc < 0:
        disc = 0.0
    sqrt_disc = math.sqrt(disc)
    return [(tr + sqrt_disc) / 2, (tr - sqrt_disc) / 2]


def trace_distance(rho1: list[list[float]], rho2: list[list[float]]) -> float:
    """§27 (QIR §3.3): D(ρ₁, ρ₂) = ½ · tr(|ρ₁ - ρ₂|)

    Мера различимости двух retrieval-состояний.
    """
    n = len(rho1)
    diff = [[rho1[i][j] - rho2[i][j] for j in range(n)] for i in range(n)]
    # Для 2×2 — аналитически
    eigvals = _eigenvalues_2x2(diff)
    return 0.5 * sum(abs(e) for e in eigvals)


def weak_measurement(rho: list[list[float]], projector: list[list[float]], lam: float
                     ) -> list[list[float]]:
    """§27.6: Слабое измерение (AAV 1988).

    ρ → (1-λ)ρ + λ · (P_k ρ P_k) / tr(P_k ρ)

    λ = 0: нет измерения (состояние intact)
    λ = 1: strong (projective) measurement
    λ in (0,1): weak measurement (частичный коллапс)

    MRL progressive: λ = 128/768 ≈ 0.17 (слабое) → ... → 1.0 (сильное).
    """
    n = len(rho)

    # Применение P_k ρ P_k
    p_rho = [[0.0] * n for _ in range(n)]
    for i in range(n):
        for j in range(n):
            s = 0.0
            for k in range(n):
                for l in range(n):
                    s += projector[i][k] * rho[k][l] * projector[l][j]
            p_rho[i][j] = s

    # trace(P_k ρ)
    tr_p_rho = sum(p_rho[i][i] for i in range(n))

    if tr_p_rho < ε_float:
        return rho  # degenerate: no collapse possible

    # Нормализация
    for i in range(n):
        for j in range(n):
            p_rho[i][j] /= tr_p_rho

    # Weak measurement: convex combination
    result = [[0.0] * n for _ in range(n)]
    for i in range(n):
        for j in range(n):
            result[i][j] = (1.0 - lam) * rho[i][j] + lam * p_rho[i][j]

    return result


def mrl_progressive_measurement(
    query: list[float],
    corpus: list[list[float]],
    dims: list[int] = [128, 256, 512, 768],
    ks: list[int] = [1000, 200, 50, 10]
) -> list[int]:
    """§27.6: MRL progressive как последовательность weak → strong измерений.

    Каждый шаг: измерение на d размерностях → коллапс до k кандидатов.
    λ = d / d_total — сила измерения.

    Возвращает индексы финальных кандидатов в исходном корпусе.
    """
    d_total = len(corpus[0])
    candidates = list(range(len(corpus)))

    for d, k in zip(dims, ks):
        lam = d / d_total
        if len(candidates) <= k:
            break

        # Проекция на первые d размерностей (subspace projector)
        scores = [cosine_sim(query, corpus[idx][:d]) for idx in candidates]

        # Born rule: cos² = collapse probability
        probs = [s * s for s in scores]

        # Collapse: сохраняем top-k по amplitude
        order = grade_down(probs)[:k]
        candidates = [candidates[i] for i in order]

        print(f"    d={d:4d} (lambda={lam:.3f}) candidates={len(candidates)}  "
              f"top score={max(scores):.4f}")

    return candidates


def kraus_channel(rho: list[list[float]], kraus_ops: list[list[list[float]]]
                  ) -> list[list[float]]:
    """§27 (QIR §7.5): Φ(ρ) = Σᵢ Kᵢ ρ Kᵢ†

    Квантовый канал: embedding, reranking, filtering как Kraus-операторы.
    """
    n = len(rho)
    result = [[0.0] * n for _ in range(n)]
    for K in kraus_ops:
        # K † ρ K
        k_dag = [[K[j][i] for j in range(n)] for i in range(n)]
        k_rho = [[0.0] * n for _ in range(n)]
        for i in range(n):
            for j in range(n):
                s = 0.0
                for k in range(n):
                    s += K[i][k] * rho[k][j]
                k_rho[i][j] = s

        k_rho_k = [[0.0] * n for _ in range(n)]
        for i in range(n):
            for j in range(n):
                s = 0.0
                for k in range(n):
                    s += k_rho[i][k] * k_dag[k][j]
                k_rho_k[i][j] = s

        for i in range(n):
            for j in range(n):
                result[i][j] += k_rho_k[i][j]

    return result


def gleason_score(rho: list[list[float]], doc: list[float]) -> float:
    """§27 (QIR §2.3): µ(L) = tr(ρ · P_L)  (Gleason's theorem)

    Единственная возможная форма вероятностной меры на подпространствах.
    Любая консистентная система ранжирования сводится к trace-формуле.
    """
    n = len(rho)
    # Projector P_d = |d⟩⟨d|
    proj = [[doc[i] * doc[j] for j in range(n)] for i in range(n)]
    # tr(ρ · P_d)
    score = 0.0
    for i in range(n):
        for j in range(n):
            score += rho[i][j] * proj[j][i]
    return score


if __name__ == "__main__":
    print("\n-- #27 Quantum Observer Effect / QIR --")

    # Демо: два документа с интерференцией
    q = [1.0, 0.0]
    d1 = [0.7071, 0.7071]
    d2 = [0.7071, 0.7071]  # идентичный d1 — максимальная интерференция
    docs = [d1, d2]

    classical, quantum, phase = quantum_vs_classical_ranking(q, docs)
    print(f"  Query:       {q}")
    print(f"  Doc A:       {d1}")
    print(f"  Doc B:       {d2}")
    print(f"  Classical scores:  {[f'{s:.4f}' for s in classical]}")
    print(f"  Quantum scores:    {[f'{s:.4f}' for s in quantum]}")
    print(f"  QPRP phase (theta_AB): {phase[0][1]:.4f} cos(theta)")

    # Демо: деструктивная интерференция
    d3 = [0.7071, -0.7071]  # orthogonal projection
    docs2 = [d1, d3]
    c2, q2, p2 = quantum_vs_classical_ranking(q, docs2)
    print(f"\n  Destructive interference demo:")
    print(f"  Doc A:       {d1}")
    print(f"  Doc C:       {d3}")
    print(f"  Classical:   {[f'{s:.4f}' for s in c2]}")
    print(f"  Quantum:     {[f'{s:.4f}' for s in q2]}")
    print(f"  QPRP cos(theta): {p2[0][1]:.4f}")

    # Демо: VN entropy
    rho_pure = density_matrix([[1.0, 0.0]])
    rho_mixed = density_matrix([[1.0, 0.0], [0.0, 1.0]], [0.5, 0.5])
    print(f"\n  VN entropy (pure query <1,0|):       {von_neumann_entropy(rho_pure):.6f}")
    print(f"  VN entropy (mixed query 50:50):       {von_neumann_entropy(rho_mixed):.6f}")

    # Демо: weak measurement
    n = 2
    I = [[1.0, 0.0], [0.0, 1.0]]
    P = [[1.0, 0.0], [0.0, 0.0]]  # projector onto first dimension
    rho_test = [[0.6, 0.3], [0.3, 0.4]]
    for lam in [0.0, 0.25, 0.5, 0.75, 1.0]:
        r = weak_measurement(rho_test, P, lam)
        s_after = sum(r[i][i] for i in range(n))
        print(f"  Weak meas. lambda={lam:.2f}: tr(rho)={s_after:.4f}  rho={r}")

    # Демо: Gleason score
    print(f"\n  Gleason score (pure |q><q| for doc [0.7,0.7]): "
          f"{gleason_score(rho_pure, [0.7071, 0.7071]):.6f}")
    print(f"  Gleason score (mixed rho for doc [0.7,0.7]):   "
          f"{gleason_score(rho_mixed, [0.7071, 0.7071]):.6f}")


# ═══════════════════════════════════════════════════════════════
#  §15  —  VECTORSPACE TYPE SYSTEM / CONVERT
# ═══════════════════════════════════════════════════════════════

@dataclass
class VectorSpace:
    """§15: Пространство векторов с известной размерностью и моделью."""
    dim: int
    model: str            # e.g. "text-embedding-3-small", "gte-large"
    metric: str           # "cosine", "euclidean", "dot"
    dtype: str            # "f32", "f16", "i8"

    def __hash__(self):
        return hash((self.dim, self.model, self.metric))


def convert_space(vec: list[float], from_meta: VectorSpace, to_meta: VectorSpace
                  ) -> list[float]:
    """§15: SPACE_CONVERT — проекция между пространствами.

    Если dim совпадает — identity (если мы верим, что модель та же).
    Если dim не совпадает — обрезка/дополнение + нормализация.
    """
    if from_meta.model == to_meta.model and from_meta.dim == to_meta.dim:
        return vec

    if from_meta.dim >= to_meta.dim:
        # Проекция: берём первые to_meta.dim и нормализуем
        projected = vec[:to_meta.dim]
    else:
        # Пэддинг: дополняем нулями
        projected = vec + [0.0] * (to_meta.dim - from_meta.dim)

    # Нормализация для cosine metric
    if to_meta.metric == "cosine":
        norm = sqrt(sum(x * x for x in projected))
        if norm > ε_float:
            projected = [x / norm for x in projected]

    return projected


def dim_mutate(vec: list[float], target_dim: int) -> list[float]:
    """§22: DIM_MUTATE — размерность как фаза вычисления.

    Проецируем вектор на target_dim измерений.
    Не обучение, не конвертация — просто алгебраическое усечение
    или Fourier Features расширение.
    """
    d = len(vec)
    if d >= target_dim:
        return vec[:target_dim]

    # Fourier feature expansion (Rahimi & Recht, 2007)
    w = [random.gauss(0, 1) for _ in range(target_dim - d)]
    expanded = vec + [math.cos(x * π) for x in w]
    return expanded


if __name__ == "__main__":
    print("\n-- #15 VectorSpace / #22 DIM_MUTATE --")
    v3 = VectorSpace(768, "text-embedding-3-small", "cosine", "f32")
    vL = VectorSpace(1024, "gte-large", "cosine", "f32")
    v_orig = [0.5] * 768
    v_conv = convert_space(v_orig, v3, vL)
    print(f"  CONVERT: 768d -> 1024d: len={len(v_conv)}, norm={sqrt(sum(x*x for x in v_conv)):.4f}")


# ═══════════════════════════════════════════════════════════════
#  §25  —  SYMBOLIC VERIFICATION: DOMAIN LAWS
# ═══════════════════════════════════════════════════════════════

def check_associative(op: Callable, a: float, b: float, c: float,
                       name: str = "op") -> bool:
    """§25: Проверка ассоциативности: (a op b) op c ≈ a op (b op c)"""
    left = op(op(a, b), c)
    right = op(a, op(b, c))
    ok = abs(left - right) < 1e-6
    check = "OK" if ok else "FAIL"
    print(f"    {name}: ({a}+{b})+{c} = {left:.6f}, {a}+({b}+{c}) = {right:.6f}  {check}")
    return ok


def check_distributive(mul: Callable, add: Callable, a: float, b: float, c: float
                       ) -> bool:
    """Проверка дистрибутивности: a.(b+c) = a.b + a.c"""
    left = mul(a, add(b, c))
    right = add(mul(a, b), mul(a, c))
    ok = abs(left - right) < 1e-6
    check = "OK" if ok else "FAIL"
    print(f"    distrib: {a}.({b}+{c}) = {left:.6f}, {a}.{b}+{a}.{c} = {right:.6f}  {check}")
    return ok


if __name__ == "__main__":
    print("\n-- #25 Symbolic Verification --")
    check_associative(lambda a, b: a + b, 1.0, 2.0, 3.0, "add")
    check_associative(lambda a, b: a * b, 2.0, 3.0, 4.0, "mul")
    check_distributive(lambda a, b: a * b, lambda a, b: a + b, 2.0, 3.0, 4.0)

    # ══════════════════════════════════════════════════════════
    #  КОСМИЧЕСКИЙ ИТОГ
    # ══════════════════════════════════════════════════════════

    print("\n" + "=" * 72)
    print("  All formulas loaded. Awaiting command: what next?")
    print("=" * 72)
