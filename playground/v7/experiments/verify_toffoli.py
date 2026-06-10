"""verify_toffoli.py — independent fidelity verification for Toffoli decomposition.

Usage:
    python verify_toffoli.py <results_log>
    python verify_toffoli.py v7/experiments/toffoli_results.log

Reads best ASTs from the experiment log, builds unitary matrices via numpy,
and computes fidelity against the Toffoli (CCNOT) target.
"""

import sys
import re
import numpy as np

# ── Gate matrices ────────────────────────────────────────────────────────────

_I = np.eye(2, dtype=complex)
_H = np.array([[1, 1], [1, -1]], dtype=complex) / np.sqrt(2)
_T = np.array([[1, 0], [0, np.exp(1j * np.pi / 4)]], dtype=complex)
_TDG = np.array([[1, 0], [0, np.exp(-1j * np.pi / 4)]], dtype=complex)
_S = np.array([[1, 0], [0, 1j]], dtype=complex)
_SDG = np.array([[1, 0], [0, -1j]], dtype=complex)
_X = np.array([[0, 1], [1, 0]], dtype=complex)
_Y = np.array([[0, -1j], [1j, 0]], dtype=complex)
_Z = np.array([[1, 0], [0, -1]], dtype=complex)
_CNOT = np.array([[1, 0, 0, 0],
                  [0, 1, 0, 0],
                  [0, 0, 0, 1],
                  [0, 0, 1, 0]], dtype=complex)

GATE_MATRICES = {
    'h': _H,
    't': _T,
    'tdg': _TDG,
    's': _S,
    'sdg': _SDG,
    'x_gate': _X,
    'y_gate': _Y,
    'z_gate': _Z,
    'cnot': _CNOT,
}

# ── Toffoli target (CCNOT, 8x8) ─────────────────────────────────────────────

_TOFFOLI = np.eye(8, dtype=complex)
_TOFFOLI[6, 6] = 0
_TOFFOLI[6, 7] = 1
_TOFFOLI[7, 6] = 1
_TOFFOLI[7, 7] = 0

# ── Unitary building ─────────────────────────────────────────────────────────

def _expand_1q(gate, qubit, n):
    """Expand a single-qubit gate to n-qubit space on index `qubit`."""
    matrices = [_I] * n
    matrices[qubit] = gate
    result = matrices[0]
    for m in matrices[1:]:
        result = np.kron(result, m)
    return result


def _expand_2q(gate, ctrl, tgt, n):
    """Expand a 2-qubit gate to n-qubit space."""
    # Build (2^n x 2^n) matrix entry by entry via permutation
    result = np.zeros((2**n, 2**n), dtype=complex)
    for r in range(2**n):
        for c in range(2**n):
            # Extract control/target bits from row and col
            bit_r_ctrl = (r >> (n - ctrl - 1)) & 1
            bit_c_ctrl = (c >> (n - ctrl - 1)) & 1
            bit_r_tgt = (r >> (n - tgt - 1)) & 1
            bit_c_tgt = (c >> (n - tgt - 1)) & 1
            gate_row = (bit_r_ctrl << 1) | bit_r_tgt
            gate_col = (bit_c_ctrl << 1) | bit_c_tgt
            # Check other qubits match
            ok = True
            for k in range(n):
                if k == ctrl or k == tgt:
                    continue
                if ((r >> (n - k - 1)) & 1) != ((c >> (n - k - 1)) & 1):
                    ok = False
                    break
            if ok:
                result[r, c] = gate[gate_row, gate_col]
    return result


def parse_ast(s):
    """Parse a Lisp-like AST string into nested lists/symbols."""
    s = s.strip()
    if s.startswith('('):
        # Split into tokens respecting nesting
        tokens = []
        depth = 0
        current = []
        for ch in s:
            if ch in (' ', '\n', '\t') and depth == 0:
                if current:
                    tokens.append(''.join(current))
                    current = []
            elif ch == '(':
                if depth > 0:
                    current.append(ch)
                depth += 1
            elif ch == ')':
                depth -= 1
                if depth == 0:
                    tokens.append(''.join(current))
                    current = []
                else:
                    current.append(ch)
            else:
                current.append(ch)
        if current:
            tokens.append(''.join(current))

        # Find the first `(` to start the recursive parse
        idx = s.index('(')
        rest = s[idx+1:]
        # Find matching close paren
        depth = 1
        i = 0
        while depth > 0 and i < len(rest):
            if rest[i] == '(':
                depth += 1
            elif rest[i] == ')':
                depth -= 1
            i += 1
        inner = rest[:i-1]
        # Parse operator
        parts = inner.split(None, 1)
        if not parts:
            return None
        op = parts[0].lower()
        args_str = parts[1] if len(parts) > 1 else ''
        # Parse arguments
        args = _parse_args(args_str) if args_str else []
        return [op] + args
    else:
        s_lower = s.lower()
        return s_lower


def _parse_args(s):
    """Parse arguments of a Lisp expression, handling nesting."""
    args = []
    i = 0
    while i < len(s):
        if s[i] in (' ', '\n', '\t'):
            i += 1
            continue
        if s[i] == '(':
            # Find matching close paren
            depth = 1
            j = i + 1
            while depth > 0 and j < len(s):
                if s[j] == '(':
                    depth += 1
                elif s[j] == ')':
                    depth -= 1
                j += 1
            inner = s[i:j]
            parsed = parse_ast(inner)
            args.append(parsed if parsed is not None else inner)
            i = j
        else:
            # Read token
            j = i
            while j < len(s) and s[j] not in (' ', '\n', '\t'):
                j += 1
            token = s[i:j]
            # Check if it's an ADF reference (ADF-N)
            if re.match(r'^ADF-\d+$', token, re.IGNORECASE):
                args.append(token.lower())
            else:
                args.append(token.lower())
            i = j
    return args


def ast_unitary(ast, n=3):
    """Build unitary from parsed AST for n qubits."""
    if isinstance(ast, str):
        return np.eye(2**n, dtype=complex)

    op = ast[0].lower()
    args = ast[1:]

    if op == 'seq':
        # seq(A, B) means B * A (apply A then B)
        if len(args) != 2:
            return np.eye(2**n, dtype=complex)
        ua = ast_unitary(args[0], n) if isinstance(args[0], list) else np.eye(2**n, dtype=complex)
        ub = ast_unitary(args[1], n) if isinstance(args[1], list) else np.eye(2**n, dtype=complex)
        return ub @ ua

    if op in GATE_MATRICES:
        gate = GATE_MATRICES[op]
        # Parse qubit args
        qubit_labels = []
        for a in args:
            if isinstance(a, str) and re.match(r'^[qQ]\d+$', a):
                qubit_labels.append(int(a[1:]) - 1)  # q1 -> 0
            elif isinstance(a, str) and a.startswith('q'):
                try:
                    qubit_labels.append(int(a[1:]) - 1)
                except ValueError:
                    return np.eye(2**n, dtype=complex)
            else:
                # invalid arg
                return np.eye(2**n, dtype=complex)

        if gate.shape == (2, 2):
            if len(qubit_labels) != 1:
                return np.eye(2**n, dtype=complex)
            return _expand_1q(gate, qubit_labels[0], n)
        elif gate.shape == (4, 4):
            if len(qubit_labels) != 2:
                return np.eye(2**n, dtype=complex)
            return _expand_2q(gate, qubit_labels[0], qubit_labels[1], n)

    return np.eye(2**n, dtype=complex)


def gate_fidelity(u_candidate, u_target=None):
    """Compute F = |Tr(u_target^dag @ u_candidate)| / N."""
    if u_target is None:
        u_target = _TOFFOLI
    n = u_target.shape[0]
    u_dag = u_target.conj().T
    product = u_dag @ u_candidate
    trace = np.trace(product)
    return np.abs(trace) / n


# ── Log parsing ──────────────────────────────────────────────────────────────

def extract_asts(log_path):
    """Extract best ASTs from 'Found valid hypotheses from Islands:' section."""
    with open(log_path, 'r', encoding='utf-8', errors='replace') as f:
        text = f.read()

    # Find the section
    match = re.search(r'Found valid hypotheses from Islands:\s*(.*?)(?:\n\n|\Z)', text, re.DOTALL)
    if not match:
        return []

    block = match.group(1)
    asts = []
    for line in block.strip().split('\n'):
        line = line.strip()
        if line.startswith('(SEQ ') or line.startswith('(seq ') or line.startswith('(S ') or line.startswith('(T ') or line.startswith('(H '):
            asts.append(line)
    return asts


# ── Main ────────────────────────────────────────────────────────────────────

def main():
    if len(sys.argv) < 2:
        log_path = "v7/experiments/toffoli_results.log"
    else:
        log_path = sys.argv[1]

    print(f"Reading: {log_path}")
    asts = extract_asts(log_path)

    if not asts:
        print("No ASTs found in log.")
        return

    print(f"\nFound {len(asts)} candidate ASTs.\n")

    best_fidelity = 0.0
    best_ast = None
    best_u = None

    for i, ast_str in enumerate(asts):
        ast = parse_ast(ast_str)
        if ast is None:
            continue
        u = ast_unitary(ast, n=3)
        f = gate_fidelity(u)

        # Check if unitary (U^dag U = I)
        is_unitary = np.allclose(u.conj().T @ u, np.eye(8), atol=1e-8)

        ok = "UNITARY" if is_unitary else "NOT_UNITARY"
        print(f"Ast #{i+1}: fidelity = {f:.8f}  [{ok}]")

        if f > best_fidelity:
            best_fidelity = f
            best_ast = ast_str
            best_u = u

    print(f"\n{'='*60}")
    print(f"Best fidelity: {best_fidelity:.8f}")
    print(f"Best AST: {best_ast}")
    print(f"{'='*60}")

    # Also compare with identity baseline
    u_identity = np.eye(8, dtype=complex)
    f_id = gate_fidelity(u_identity)
    print(f"Identity baseline fidelity: {f_id:.8f} (expected 0.75)")

    # Perfect Toffoli
    f_perfect = gate_fidelity(_TOFFOLI)
    print(f"Perfect Toffoli fidelity:  {f_perfect:.8f} (expect 1.0)")

    if best_fidelity > 0.999:
        print("\n[OK] Toffoli decomposition FOUND with fidelity > 0.999")
    elif best_fidelity > 0.99:
        print(f"\n[OK] High fidelity ({best_fidelity:.4f}), but not exact")
    elif best_fidelity > 0.9:
        print(f"\n[OK] Good approximation ({best_fidelity:.4f})")
    else:
        print(f"\n[FAIL] Poor fidelity ({best_fidelity:.4f}) -- search needs improvement")


if __name__ == '__main__':
    main()
