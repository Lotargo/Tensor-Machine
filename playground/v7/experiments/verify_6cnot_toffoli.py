"""Verify 6-CNOT Toffoli decomposition from Qiskit CCXGate source."""
import numpy as np
import sys
sys.path.insert(0, '.')
from find_correct_toffoli import _I, _H, _T, _TDG, _S, _SDG, _CNOT, _TARGET, e1, e2, fid, build

# From Qiskit CCXGate._define() comment:
# q_0: ───────────────────■─────────────────────■────■───┤ T ├───■──
# q_1: ───────■───────────┼─────────■───┤ T ├───┼──┤ X ├┤ Tdg ├┤ X ├
# q_2: ┤ H ├┤ X ├┤ Tdg ├┤ X ├┤ T ├┤ X ├┤ Tdg ├┤ X ├┤ T ├─┤ H ├──────

# Interpreted left-to-right:
gates_6cnot = [
    ('h', 2),           # H on q2
    ('c', 0, 2),        # CNOT(q0, q2)
    ('td', 2),          # Tdg on q2
    ('c', 1, 2),        # CNOT(q1, q2)
    ('t', 2),           # T on q2
    ('c', 0, 2),        # CNOT(q0, q2)
    ('td', 2),          # Tdg on q2
    ('c', 1, 2),        # CNOT(q1, q2)
    ('t', 2),           # T on q2
    ('t', 1),           # T on q1 (parallel with T q2)
    ('c', 0, 1),        # CNOT(q0, q1)
    ('td', 1),          # Tdg on q1
    ('c', 0, 1),        # CNOT(q0, q1)
    ('t', 0),           # T on q0
    ('h', 2),           # H on q2
]

U = build(gates_6cnot)
f = fid(U)
print(f"6-CNOT Toffoli Fidelity: {f:.10f}")
print(f"Exact match: {np.allclose(U, _TARGET)}")
diff = np.max(np.abs(U - _TARGET))
print(f"Max diff: {diff:.2e}")

# Count gates
cnots = sum(1 for g in gates_6cnot if g[0] == 'c')
tcount = sum(1 for g in gates_6cnot if g[0] in ('t', 'td'))
print(f"CNOT count: {cnots}, T-count: {tcount}")

# Also verify: is this the same as the 4-CNOT version but with extra CNOTs?
# Let's test permutation
for inp in range(8):
    v = np.zeros(8); v[inp] = 1
    out = U @ v
    out_idx = np.argmax(np.abs(out))
    expected = inp
    if inp == 6: expected = 7
    elif inp == 7: expected = 6
    status = "✓" if out_idx == expected else "✗"
    print(f"  |{inp:03b}⟩ -> |{out_idx:03b}⟩ {status}")
