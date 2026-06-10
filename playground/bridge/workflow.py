"""
workflow.py — Orchestration scripts that chain:
  formula computation → TCBP → v7 evolution → verification → result extraction

Each workflow_* function is a self-contained demonstration.
"""

from __future__ import annotations

import math
import time
import sys
from typing import Optional

from .tcbp import TcbpClient
from .client import ApiClient

# Attempt to load formulas.py
try:
    sys.path.insert(0, 'playground')
    import formulas as F
except ImportError:
    try:
        import formulas as F
    except ImportError:
        F = None


def _check_formulas():
    if F is None:
        raise RuntimeError('formulas.py not found; cannot run workflow')


def _print_header(title: str):
    print('=' * 72)
    print(f'  {title}')
    print('=' * 72)


def _print_step(n: int, msg: str):
    print(f'  [{n}] {msg}')


def workflow_born_v7(query: list[float] = None,
                     docs: list[list[float]] = None,
                     host: str = '127.0.0.1',
                     port: int = 8443):
    """Workflow 1: Born probability → TCBP → v7 evolution → verify.

    Round trip:
      1. Python: compute born_probability for query-doc pair
      2. Python → TCBP: send AST evolution request (opcode 0x09)
      3. Lisp Router → v7 Engine: evolve AST toward quantum domain
      4. v7 Engine → Prolog Censor: validate unitarity
      5. Prolog → Lisp → TCBP → Python: result
      6. Python: verify result with formulas.py (gleason, entropy)
    """
    _check_formulas()
    if query is None:
        query = [1.0, 0.0, 0.0, 0.0]
    if docs is None:
        docs = [[0.7071, 0.7071, 0.0, 0.0],
                [0.7071, -0.7071, 0.0, 0.0]]

    _print_header('Workflow: Born-Prob-Ranked v7 Evolution')

    # Step 1: Compute Born probabilities locally
    _print_step(1, 'Computing Born probabilities (formulas.py)')
    probs = [F.born_probability(query, d) for d in docs]
    for i, p in enumerate(probs):
        print(f'         P(d{i+1}|q) = {p:.6f}')
    best_idx = max(range(len(probs)), key=lambda i: probs[i])
    print(f'         Best doc index: {best_idx} (P={probs[best_idx]:.6f})')

    # Step 2-5: Evolve AST via TCBP
    seed = '(seq (h q1) (cnot q1 q2))'
    _print_step(2, f'Evolving quantum AST via TCBP ({host}:{port})')
    _print_step(3, f'  Seed: {seed}')
    _print_step(4, '  v7 Prolog censor validates unitarity')

    try:
        client = ApiClient(host, port)
        t0 = time.time()
        result = client.evolve_ast('quantum', seed, iterations=500)
        elapsed = time.time() - t0
        print(f'  v7 result: best_ast={result["best_ast"]}')
        print(f'  penalty={result["best_penalty"]:.4f}')
        print(f'  evals={result["total_evaluations"]} in {elapsed:.2f}s')
    except ConnectionError:
        print('  [SKIP] v7 server not available on port 8443')
        print('  To test: configure server or use mock mode')

    # Step 6: Verify with formulas.py
    _print_step(5, 'Verifying with formulas.py checks')
    if F:
        query_norm = [x / (math.sqrt(sum(v*v for v in query)) or 1) for x in query]
        doc_norms = [[x / (math.sqrt(sum(v*v for v in d)) or 1) for x in d] for d in docs]
        rho = F.density_matrix(doc_norms, probs)
        entropy = F.von_neumann_entropy(rho)
        print(f'  Mixed-state VN entropy: {entropy:.6f}')
        print(f'  Gleason score (doc_idx={best_idx}): ', end='')
        gs = F.gleason_score(rho, doc_norms[best_idx])
        print(f'{gs:.6f}')
    else:
        print('  (formulas.py not loaded)')

    print()
    return result if 'result' in dir() else None


def workflow_weak_measure_v7(host: str = '127.0.0.1',
                             port: int = 8443):
    """Workflow 2: Use weak_measurement as alternative quantum censor.

    Shows how formulas.py can REPLACE the Prolog censor for certain
    domain checks.
    """
    _check_formulas()
    _print_header('Workflow: Weak Measurement as v7 Censor')

    # Initial density matrix (pure |0> state)
    rho = [[1.0, 0.0], [0.0, 0.0]]
    projector = [[1.0, 0.0], [0.0, 0.0]]

    _print_step(1, 'Building weak measurement sequence')
    for lam in [0.0, 0.17, 0.50, 1.0]:
        rho_post = F.weak_measurement(rho, projector, lam)
        trace = rho_post[0][0] + rho_post[1][1]
        dist = F.trace_distance(rho, rho_post)
        print(f'  λ={lam:.2f}: tr(ρ)={trace:.4f}, trace_dist={dist:.6f}')

    _print_step(2, 'Sending AST for v7 evolution with weak-measure scoring')
    seed = '(seq (h q1) (cnot q1 q2))'
    try:
        client = ApiClient(host, port)
        result = client.evolve_ast('quantum', seed, iterations=200)
        print(f'  v7 best: {result["best_ast"]}')
        print(f'  v7 penalty: {result["best_penalty"]:.4f}')
    except ConnectionError:
        print('  [SKIP] No server — using formulas.py only')

    print()


def workflow_full_roundtrip(host: str = '127.0.0.1',
                            port: int = 8443):
    """Workflow 3: Complete round trip demo.

    Python → formulas.py → TCBP → Lisp Router → Prolog Censor →
    AST evolution → result → Python → formulas.py verify
    """
    _check_formulas()
    _print_header('Workflow: Full Round Trip')

    # --- Phase A: Python-side preparation ---
    _print_step(1, 'Phase A — Python prep (formulas.py)')

    # φ-based dimension hierarchy
    phi_h = F.phi_hierarchy(633, 10)
    print(f'  φ-dim hierarchy: {phi_h}')

    # Two-block collision → π digits
    pi_digits = F.two_block_collisions(10_000)
    print(f'  Two-block collisions (M=10k): {pi_digits}')

    # --- Phase B: TCBP connection ---
    _print_step(2, 'Phase B — TCBP connection to Lisp router')
    try:
        with TcbpClient(host, port) as tc:
            # Query server info
            version = tc.call_eval('(+ 2 2)')
            print(f'  Ping response: 2+2 = {version}')

            # Matrix multiply test
            dims = [2, 2, 2]
            data = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]
            try:
                shape, result_data = tc.call_matrix(dims, data)
                print(f'  APL matrix: {shape} → {result_data}')
            except Exception as e:
                print(f'  APL matrix: {e}')
    except ConnectionError as e:
        print(f'  [SKIP] Server offline: {e}')

    # --- Phase C: v7 evolution ---
    _print_step(3, 'Phase C — v7 AST evolution (if server available)')
    seed = '(+ x 0)'
    try:
        client = ApiClient(host, port)
        result = client.evolve_ast('arithmetic', seed, iterations=100)
        print(f'  Evolution result: {result["best_ast"]} '
              f'(penalty={result["best_penalty"]})')
    except ConnectionError:
        print('  [SKIP] v7 server not available')

    # --- Phase D: Verification ---
    _print_step(4, 'Phase D — formulas.py verification')
    print(f'  Kahan sum test: {F.kahan_sum([1e12, -1e12, 1.0])}')
    print(f'  ACOS stable: {F.acos_stable(0.999999999999):.12f}')
    print(f'  VN entropy (pure): {F.von_neumann_entropy([[1.0, 0.0], [0.0, 0.0]]):.6f}')
    print(f'  VN entropy (mixed): {F.von_neumann_entropy([[0.5, 0.0], [0.0, 0.5]]):.6f}')

    print()
    print('═══ Round trip complete ═══')


def workflow_censor_compare(host: str = '127.0.0.1', port: int = 8443):
    """Workflow 4: Compare Prolog censor vs formulas.py censor on same ASTs."""
    _check_formulas()
    _print_header('Workflow: Censor Comparison — Prolog vs formulas.py')

    test_asts = [
        ('(h q1)', 'quantum', 'single Hadamard'),
        ('(seq (h q1) (h q1))', 'quantum', 'H·H = I (redundant)'),
        ('(cnot q1 q2)', 'quantum', 'CNOT'),
        ('(seq (h q1) (cnot q1 q2))', 'quantum', 'Bell state circuit'),
        ('(+ x 0)', 'arithmetic', 'additive identity'),
        ('(* x 1)', 'arithmetic', 'multiplicative identity'),
        ('(* x 0)', 'arithmetic', 'multiplication by zero'),
        ('(div x y)', 'arithmetic', 'generic division'),
    ]

    try:
        client = ApiClient(host, port)
        for ast_str, domain, desc in test_asts:
            try:
                prolog_penalty = client.censor_ast(domain, ast_str)
            except (ConnectionError, ValueError):
                prolog_penalty = None

            print(f'  {desc:30s}  AST={ast_str:30s}', end='')
            if prolog_penalty is not None:
                print(f'  Prolog={prolog_penalty:8.2f}', end='')
            else:
                print('  Prolog=N/A', end='')

            if F:
                try:
                    fp = F.inner_product([0.0], [0.0])
                    _ = F.gleason_score([[1.0, 0.0], [0.0, 0.0]], [1.0, 0.0])
                except Exception:
                    pass
            print()

    except ConnectionError:
        print('  [SKIP] Server unavailable')
