"""
client.py — High-level API wrapping TCBP calls with numpy types.

Each function takes numpy arrays or Python types, serialises to TCBP,
sends to the Lisp router, and decodes the result back to numpy/Python.
"""

from __future__ import annotations

import numpy as np
from typing import Optional

from .tcbp import TcbpClient, Opcode, pack_matrix_payload, parse_matrix_payload
from .tcbp import pack_complex_payload, parse_complex_payload, pack_csr_payload
from .tcbp import pack_evolve_payload, pack_censor_payload


class ApiClient:
    """High-level API client for the hybrid v7+v10 compute system.

    Wraps TcbpClient with numpy-aware input/output.
    """

    def __init__(self, host: str = '127.0.0.1', port: int = 8443,
                 v7_port: Optional[int] = None):
        self._tcbp = TcbpClient(host, port)
        self._v7_port = v7_port or port  # same router if v7 ops are added
        self._v7_tcbp: Optional[TcbpClient] = None

    def _v7_client(self) -> TcbpClient:
        if self._v7_tcbp is None:
            self._v7_tcbp = TcbpClient(self._tcbp.host, self._v7_port)
        return self._v7_tcbp

    def close(self):
        self._tcbp.close()
        if self._v7_tcbp is not None:
            self._v7_tcbp.close()

    # ------------------------------------------------------------------
    # Core math operations (→ v10 router)
    # ------------------------------------------------------------------

    def eval_expr(self, expression: str) -> str:
        """Evaluate a safe arithmetic expression on the Lisp server."""
        return self._tcbp.call_eval(expression)

    def matrix_multiply(self, A: np.ndarray, B: np.ndarray) -> np.ndarray:
        """Multiply two matrices via the APL coprocessor.

        Falls back to Lisp blocked multiply if APL unavailable.
        """
        a_flat = np.ascontiguousarray(A, dtype=np.float64).ravel()
        b_flat = np.ascontiguousarray(B, dtype=np.float64).ravel()
        shape = [A.shape[0], A.shape[1], B.shape[1]]
        payload = pack_matrix_payload(shape, np.concatenate([a_flat, b_flat]))
        resp = self._tcbp.send_frame(Opcode.MATRIX, payload)
        dims, data = parse_matrix_payload(resp.payload)
        return np.array(data, dtype=np.float64).reshape(dims)

    def solve_constraints(self, equations: str) -> str:
        """Solve a system of equations via Prolog CLP(FD).

        Format: 'X+Y=10,X-Y=2'
        """
        return self._tcbp.call_solve(equations)

    def qsim_circuit(self, state_vector: np.ndarray) -> np.ndarray:
        """Apply Hadamard gate to a quantum state vector.

        state_vector: complex128 array of length 2^n.
        Returns new state vector.
        """
        real = np.real(state_vector).tolist()
        imag = np.imag(state_vector).tolist()
        out_real, out_imag = self._tcbp.call_qsim(real, imag)
        return np.array(out_real, dtype=np.float64) + 1j * np.array(out_imag, dtype=np.float64)

    def dsp_fft(self, signal: np.ndarray) -> np.ndarray:
        """Compute DFT (naive or FFT) of a complex signal via Lisp server.

        signal: complex128 array.
        Returns complex128 frequency-domain representation.
        """
        real = np.real(signal).tolist()
        imag = np.imag(signal).tolist()
        out_real, out_imag = self._tcbp.call_dsp(real, imag)
        return np.array(out_real, dtype=np.float64) + 1j * np.array(out_imag, dtype=np.float64)

    def symbolic_diff(self, func: str, var: str) -> str:
        """Differentiate a symbolic expression with respect to var."""
        return self._tcbp.call_sym(func, var)

    def shortest_path(self, adj_matrix: np.ndarray, source: int = 0
                      ) -> tuple[np.ndarray, np.ndarray]:
        """Dijkstra shortest path on a dense adjacency matrix.

        Internally converts to CSR format.
        """
        n = adj_matrix.shape[0]
        nnz = int(np.isfinite(adj_matrix).sum())
        row_ptrs = [0]
        col_indices = []
        data = []
        for i in range(n):
            for j in range(n):
                if np.isfinite(adj_matrix[i, j]):
                    col_indices.append(j)
                    data.append(float(adj_matrix[i, j]))
            row_ptrs.append(len(col_indices))
        distances, predecessors = self._tcbp.call_graph(
            n, n, row_ptrs, col_indices, data)
        return (np.array(distances, dtype=np.float64),
                np.array(predecessors, dtype=np.float64))

    # ------------------------------------------------------------------
    # v7 evolution operations (→ v7 engine over TCBP)
    # ------------------------------------------------------------------

    def evolve_ast(self, domain: str, seed_ast: str,
                   iterations: int = 500, flags: int = 0) -> dict:
        """Evolve an AST toward a domain target via v7 engine.

        Args:
            domain: one of 'quantum', 'arithmetic', 'calculus', 'relativity',
                    'thermo', 'holography', 'logic', 'regression', 'synthesis'
            seed_ast: Lisp S-expression string, e.g. '(seq (h q1) (cnot q1 q2))'
            iterations: max generations
            flags: bit 0 = enable novelty search, bit 1 = enable Sonata

        Returns:
            dict with best_ast, best_penalty, total_evaluations, elapsed_seconds
        """
        return self._v7_client().call_evolve(domain, seed_ast, iterations, flags)

    def censor_ast(self, domain: str, ast: str) -> float:
        """Pass an AST through the Prolog censor without evolution.

        Returns penalty float (0.0 = perfect, 1000.0 = rejected).
        """
        return self._v7_client().call_censor(domain, ast)
