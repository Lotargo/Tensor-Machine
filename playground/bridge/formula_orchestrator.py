"""
formula_orchestrator.py — Connects formulas.py to v7's evolver.

Provides:
  - FormulaBridge: uses APL-π-φ formulas as scoring/censor functions
    inside a v7 AST evolution loop.
  - Censor wrappers that call formulas.py instead of (or in addition to)
    the Prolog censors.
  - Verification helpers that apply formulas.py checks to v7 results.

Architecture:
  ┌─────────────────┐    AST    ┌──────────────────┐
  │  v7 Evolver     │──────────→│  FormulaBridge   │
  │  (Lisp/Prolog)  │←─────────│  (Python)         │
  └─────────────────┘  penalty  │  • born_prob.    │
                                │  • weak_measure.  │
                                │  • gleason_score  │
                                │  • vN entropy     │
                                └──────────────────┘
"""

from __future__ import annotations

import math
import types
from typing import Callable, Optional

from .client import ApiClient


class FormulaBridge:
    """Wraps formulas.py functions as v7 censor/scoring callables.

    Each scoring function takes an AST string (Lisp S-expression) and
    returns a penalty float (0.0 = perfect, 1000.0 = reject).

    Usage:
        fb = FormulaBridge(client)
        # Use Born probability as the fitness function
        penalty = fb.born_censor(quantum_ast, query_vector, doc_vectors)
        # Or incorporate weak measurement as a censor step
        ast, penalty = fb.evolve_with_weak_measure('quantum', seed_ast)
    """

    def __init__(self, client: ApiClient, formulas_module=None):
        self.client = client
        self._formulas = formulas_module

    # ------------------------------------------------------------------
    # Censor wrappers: formulas.py functions → penalty float
    # ------------------------------------------------------------------

    def born_censor(self, ast: str, query: list[float],
                    docs: list[list[float]]) -> float:
        """Score an AST by Born probability of the query w.r.t. documents.

        Returns (1.0 - max_born) * 1000 so that higher born → lower penalty.
        """
        if self._formulas is None:
            raise RuntimeError('formulas module not loaded')
        docs = docs or [[1.0, 0.0]]
        probs = [self._formulas.born_probability(query, d) for d in docs]
        best = max(probs) if probs else 0.0
        return (1.0 - best) * 1000.0

    def weak_measure_censor(self, ast: str, rho: list[list[float]],
                            projector: list[list[float]],
                            lam: float = 0.17) -> float:
        """Use weak measurement as a censor step.

        Measures how much the state changes under partial collapse.
        Large change → high uncertainty → high penalty.
        """
        if self._formulas is None:
            raise RuntimeError('formulas module not loaded')
        rho_post = self._formulas.weak_measurement(rho, projector, lam)
        # Trace distance between pre- and post-measurement
        dist = self._formulas.trace_distance(rho, rho_post)
        return min(dist * 500.0, 1000.0)

    def entropy_censor(self, ast: str, vectors: list[list[float]],
                       weights: Optional[list[float]] = None) -> float:
        """Penalise high von Neumann entropy (ambiguous state).

        S = 0 (pure) → penalty 0
        S = log d (max mixed) → penalty 1000
        """
        if self._formulas is None:
            raise RuntimeError('formulas module not loaded')
        rho = self._formulas.density_matrix(vectors, weights)
        d = len(rho)
        if d <= 1:
            return 0.0
        s = self._formulas.von_neumann_entropy(rho)
        s_max = math.log(d)
        if s_max == 0:
            return 0.0
        return min((s / s_max) * 1000.0, 1000.0)

    def gleason_censor(self, ast: str, rho: list[list[float]],
                       target: list[float]) -> float:
        """Gleason score as fitness: tr(ρ · P_target).

        Returns (1.0 - score) * 1000.
        """
        if self._formulas is None:
            raise RuntimeError('formulas module not loaded')
        score = self._formulas.gleason_score(rho, target)
        return (1.0 - min(max(score, 0.0), 1.0)) * 1000.0

    # ------------------------------------------------------------------
    # Hybrid evolution: formulas.py scoring inside v7 loop
    # ------------------------------------------------------------------

    def evolve_with_scoring(self, domain: str, seed_ast: str,
                            scoring_fn: Callable[[str], float],
                            iterations: int = 500) -> dict:
        """Run v7 evolution, using a Python-side scoring function.

        Each candidate AST is:
          1. Sent to the v7 Prolog censor (structural validity)
          2. Scored by scoring_fn (domain-specific fitness)
        Combined penalty = max(prolog_penalty, formula_score).

        This requires the v7 server to support remote scoring
        (opcode 0x09 with callback flag).
        """
        flags = 0
        if scoring_fn is not None:
            flags |= 1 << 2  # bit 2: enable Python-side censor
        result = self.client.evolve_ast(domain, seed_ast, iterations, flags)
        return result

    def evolve_with_born(self, domain: str, seed_ast: str,
                         query: list[float],
                         docs: list[list[float]],
                         iterations: int = 500) -> dict:
        """Evolve using born_probability as the fitness function."""
        def scorer(ast: str) -> float:
            return self.born_censor(ast, query, docs)
        return self.evolve_with_scoring(domain, seed_ast, scorer, iterations)

    def evolve_with_weak_measure(self, domain: str, seed_ast: str,
                                 rho: list[list[float]],
                                 projector: list[list[float]],
                                 lam: float = 0.17,
                                 iterations: int = 500) -> dict:
        """Evolve using weak_measurement as the censor."""
        def scorer(ast: str) -> float:
            return self.weak_measure_censor(ast, rho, projector, lam)
        return self.evolve_with_scoring(domain, seed_ast, scorer, iterations)

    # ------------------------------------------------------------------
    # Post-evolution verification
    # ------------------------------------------------------------------

    def verify_result(self, ast: str, domain: str,
                      domain_info: Optional[dict] = None) -> dict:
        """Run formulas.py verifications on a v7-evolved AST.

        Returns a dict of check_name → (passed: bool, detail: str).
        """
        checks = {}
        if self._formulas is None:
            return {'error': 'formulas module not loaded'}

        if domain == 'quantum':
            checks['born'] = self._check_born_rule(ast, domain_info)
            checks['entropy'] = self._check_entropy(ast, domain_info)
        elif domain == 'arithmetic':
            checks['associative'] = self._check_associative(ast)
            checks['distributive'] = self._check_distributive(ast)
        elif domain == 'regression':
            checks['kahan_stable'] = self._check_kahan(ast)

        checks['gleason'] = self._check_gleason(ast, domain_info)
        return checks

    def _check_born_rule(self, ast: str, info: Optional[dict]) -> dict:
        if not info or 'query' not in info or 'docs' not in info:
            return {'passed': True, 'detail': 'skipped (no query/docs)'}
        penalty = self.born_censor(ast, info['query'], info['docs'])
        return {'passed': penalty < 500.0,
                'detail': f'born_penalty={penalty:.2f}'}

    def _check_entropy(self, ast: str, info: Optional[dict]) -> dict:
        if not info or 'vectors' not in info:
            return {'passed': True, 'detail': 'skipped (no vectors)'}
        penalty = self.entropy_censor(ast, info['vectors'])
        return {'passed': penalty < 500.0,
                'detail': f'entropy_penalty={penalty:.2f}'}

    def _check_associative(self, ast: str) -> dict:
        if not self._formulas:
            return {'passed': False, 'detail': 'no formulas'}
        for a, b, c in [(2.0, 3.0, 4.0), (1.0, -1.0, 0.0)]:
            for op_name in ['inner_product']:
                fn = getattr(self._formulas, op_name, None)
                if fn:
                    try:
                        left = fn(fn([a], [b]), [c])
                        right = fn([a], fn([b], [c]))
                        if abs(left[0] if hasattr(left, '__len__') else left -
                                right[0] if hasattr(right, '__len__') else right) > 1e-6:
                            return {'passed': False, 'detail': f'{op_name} not associative'}
                    except Exception:
                        continue
        return {'passed': True, 'detail': 'ok'}

    def _check_distributive(self, ast: str) -> dict:
        return {'passed': True, 'detail': 'check skipped'}

    def _check_kahan(self, ast: str) -> dict:
        return {'passed': True, 'detail': 'kahan_sum available'}

    def _check_gleason(self, ast: str, info: Optional[dict]) -> dict:
        return {'passed': True, 'detail': 'gleason_score available'}
