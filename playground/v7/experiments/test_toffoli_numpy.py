"""Verify known Toffoli decomposition with numpy."""
import numpy as np

# Gate matrices
_I = np.eye(2, dtype=complex)
_H = np.array([[1, 1], [1, -1]], dtype=complex) / np.sqrt(2)
_T = np.array([[1, 0], [0, np.exp(1j * np.pi / 4)]], dtype=complex)
_TDG = np.array([[1, 0], [0, np.exp(-1j * np.pi / 4)]], dtype=complex)
_X = np.array([[0, 1], [1, 0]], dtype=complex)
_CNOT = np.array([[1, 0, 0, 0],
                  [0, 1, 0, 0],
                  [0, 0, 0, 1],
                  [0, 0, 1, 0]], dtype=complex)

def expand_1q(gate, qubit, n=3):
    mats = [_I] * n
    mats[qubit] = gate
    result = mats[0]
    for m in mats[1:]:
        result = np.kron(result, m)
    return result

def expand_2q(gate, ctrl, tgt, n=3):
    result = np.zeros((2**n, 2**n), dtype=complex)
    for r in range(2**n):
        for c in range(2**n):
            bit_r_ctrl = (r >> (n - ctrl - 1)) & 1
            bit_c_ctrl = (c >> (n - ctrl - 1)) & 1
            bit_r_tgt = (r >> (n - tgt - 1)) & 1
            bit_c_tgt = (c >> (n - tgt - 1)) & 1
            gate_row = (bit_r_ctrl << 1) | bit_r_tgt
            gate_col = (bit_c_ctrl << 1) | bit_c_tgt
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

# Toffoli target
_TOFFOLI = np.eye(8, dtype=complex)
_TOFFOLI[6, 6] = 0
_TOFFOLI[6, 7] = 1
_TOFFOLI[7, 6] = 1
_TOFFOLI[7, 7] = 0

def gate_fidelity(U_candidate, U_target=None):
    if U_target is None:
        U_target = _TOFFOLI
    n = U_target.shape[0]
    U_dag = U_target.conj().T
    product = U_dag @ U_candidate
    trace = np.trace(product)
    return np.abs(trace) / n

# Build the known Toffoli decomposition
# Standard: H q3 · T† q3 · CNOT q2,q3 · T q3 · CNOT q1,q3 · T† q3 · CNOT q2,q3 · T q3 · T† q2 · CNOT q1,q2 · T q2 · T q1 · H q3
# First gate applied is h(q3), last is h(q3)

gates = [
    ('h', 2),           # h(q3) — first applied
    ('tdg', 2),         # tdg(q3)
    ('cnot', 1, 2),     # cnot(q2,q3) — ctrl=q2(idx1), tgt=q3(idx2)
    ('t', 2),           # t(q3)
    ('cnot', 0, 2),     # cnot(q1,q3) — ctrl=q1(idx0), tgt=q3(idx2)
    ('tdg', 2),         # tdg(q3)
    ('cnot', 1, 2),     # cnot(q2,q3)
    ('t', 2),           # t(q3)
    ('tdg', 1),         # tdg(q2)
    ('cnot', 0, 1),     # cnot(q1,q2)
    ('t', 1),           # t(q2)
    ('t', 0),           # t(q1)
    ('h', 2),           # h(q3) — last applied
]

U = np.eye(8, dtype=complex)
for g in gates:
    if g[0] == 'h':
        Ug = expand_1q(_H, g[1])
    elif g[0] == 't':
        Ug = expand_1q(_T, g[1])
    elif g[0] == 'tdg':
        Ug = expand_1q(_TDG, g[1])
    elif g[0] == 'cnot':
        Ug = expand_2q(_CNOT, g[1], g[2])
    else:
        raise ValueError(f"Unknown gate: {g[0]}")
    U = Ug @ U  # Apply gate (new gate left-multiplies current unitary)

print(f"Final unitary shape: {U.shape}")
fidelity = gate_fidelity(U)
print(f"Fidelity vs Toffoli: {fidelity:.8f}")

# Check unitarity
is_unitary = np.allclose(U.conj().T @ U, np.eye(8), atol=1e-8)
print(f"Is unitary: {is_unitary}")

# Print a few rows to check
print(f"\nDiagonal of U: {np.diag(U)}")
print(f"Trace of U: {np.trace(U):.4f}")

# Compare with Toffoli
diff = U - _TOFFOLI
print(f"Max diff from Toffoli: {np.max(np.abs(diff)):.6f}")

# Also test: what if the decomposition is wrong?
# Try the reversed order (last gate applied first)
print("\n--- Testing with reversed gate order ---")
U2 = np.eye(8, dtype=complex)
for g in reversed(gates):
    if g[0] == 'h':
        Ug = expand_1q(_H, g[1])
    elif g[0] == 't':
        Ug = expand_1q(_T, g[1])
    elif g[0] == 'tdg':
        Ug = expand_1q(_TDG, g[1])
    elif g[0] == 'cnot':
        Ug = expand_2q(_CNOT, g[1], g[2])
    U2 = Ug @ U2

fidelity2 = gate_fidelity(U2)
print(f"Fidelity (reversed): {fidelity2:.8f}")
diff2 = U2 - _TOFFOLI
print(f"Max diff from Toffoli: {np.max(np.abs(diff2)):.6f}")

# Try alternative: is it a known different decomposition?
# Check if U equals CNOT(q1,q2) * Toffoli
print("\n--- Check if known decomposition is correct (might use different convention) ---")
# Test: maybe CNOT control/target are swapped?
# Or maybe the decomposition is actually different

# Let me verify by checking individual gate effects
# |110> = |6> should map to |111> = |7>
input_6 = np.zeros(8); input_6[6] = 1
output = U @ input_6
print(f"|110> -> |{np.argmax(np.abs(output))}> (prob={np.max(np.abs(output))**2:.4f})")

# |111> = |7> should map to |110> = |6>
input_7 = np.zeros(8); input_7[7] = 1
output = U @ input_7
print(f"|111> -> |{np.argmax(np.abs(output))}> (prob={np.max(np.abs(output))**2:.4f})")
