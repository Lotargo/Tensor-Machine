"""Find correct Toffoli decomposition with numpy."""
import numpy as np

_I = np.eye(2, dtype=complex)
_H = np.array([[1, 1], [1, -1]], dtype=complex) / np.sqrt(2)
_T = np.array([[1, 0], [0, np.exp(1j * np.pi / 4)]], dtype=complex)
_TDG = np.array([[1, 0], [0, np.exp(-1j * np.pi / 4)]], dtype=complex)
_S = np.array([[1, 0], [0, 1j]], dtype=complex)
_SDG = np.array([[1, 0], [0, -1j]], dtype=complex)
_CNOT = np.array([[1, 0, 0, 0],
                  [0, 1, 0, 0],
                  [0, 0, 0, 1],
                  [0, 0, 1, 0]], dtype=complex)

def e1(g, q, n=3):
    ms = [_I]*n; ms[q]=g; r=ms[0]
    for m in ms[1:]: r=np.kron(r,m)
    return r

def e2(g, c, t, n=3):
    r = np.zeros((2**n,2**n), dtype=complex)
    for i in range(2**n):
        for j in range(2**n):
            bic=(i>>(n-c-1))&1; bjc=(j>>(n-c-1))&1
            bit=(i>>(n-t-1))&1; bjt=(j>>(n-t-1))&1
            gr=(bic<<1)|bit; gc=(bjc<<1)|bjt
            ok = all(((i>>(n-k-1))&1)==((j>>(n-k-1))&1)
                     for k in range(n) if k not in (c,t))
            if ok: r[i,j]=g[gr,gc]
    return r

_TARGET = np.eye(8, dtype=complex)
_TARGET[6,6]=0; _TARGET[6,7]=1; _TARGET[7,6]=1; _TARGET[7,7]=0

def build(gs):
    U = np.eye(8, dtype=complex)
    for g in gs:
        if g[0]=='h': Ug=e1(_H,g[1])
        elif g[0]=='t': Ug=e1(_T,g[1])
        elif g[0]=='td': Ug=e1(_TDG,g[1])
        elif g[0]=='s': Ug=e1(_S,g[1])
        elif g[0]=='sd': Ug=e1(_SDG,g[1])
        elif g[0]=='c': Ug=e2(_CNOT,g[1],g[2])
        else: raise ValueError(g[0])
        U = Ug @ U
    return U

def fid(U, target=None):
    if target is None: target=_TARGET
    p = target.conj().T @ U
    return np.abs(np.trace(p))/8

variants = {
    # Maslov 2012 decomposition
    "Maslov": [
        ('h',2),('c',1,2),('td',2),('c',0,2),('t',2),
        ('c',1,2),('td',2),('c',0,2),('t',2),
        ('td',1),('c',0,1),('t',1),('t',0),('h',2)],
    # Amy, Maslov, Mosca 2013
    "AMM2013": [
        ('h',2),('c',1,2),('t',2),('c',0,2),('td',2),
        ('c',1,2),('t',2),('c',0,2),('td',2),
        ('t',1),('c',0,1),('td',1),('t',0),('h',2)],
    # Nielsen & Chuang variant a
    "NC_a": [
        ('h',2),('c',1,2),('td',2),('c',0,2),('t',2),
        ('c',1,2),('td',2),('c',0,2),('t',2),
        ('t',1),('c',0,1),('td',1),('t',0),('h',2)],
    # Nielsen & Chuang variant b
    "NC_b": [
        ('h',2),('c',1,2),('td',2),('c',0,2),('t',2),
        ('c',1,2),('td',2),('c',0,2),('t',2),
        ('t',1),('c',0,1),('t',0),('h',2)],
    # What if T/TDG have S embedded? (S gate variant)
    "With_S": [
        ('h',2),('c',1,2),('td',2),('c',0,2),('s',2),
        ('c',1,2),('td',2),('c',0,2),('t',2),
        ('td',1),('c',0,1),('t',1),('t',0),('h',2)],
    # What if TD/T alternation is different?
    "Alt_TD_T": [
        ('h',2),('c',1,2),('t',2),('c',0,2),('td',2),
        ('c',1,2),('t',2),('c',0,2),('td',2),
        ('td',1),('c',0,1),('t',1),('t',0),('h',2)],
    # Qiskit CCXGate decomposition (6 CNOT, verified Fidelity=1.0)
    # From qiskit.circuit.library.standard_gates.x.py CCXGate._define()
    "Qiskit_6CNOT": [
        ('h',2), ('c',0,2), ('td',2), ('c',1,2), ('t',2),
        ('c',0,2), ('td',2), ('c',1,2), ('t',2),
        ('t',1), ('c',0,1), ('td',1), ('c',0,1), ('t',0), ('h',2)],

    # Variant without ending H
    "NoEndH": [
        ('h',2),('c',1,2),('td',2),('c',0,2),('t',2),
        ('c',1,2),('td',2),('c',0,2),('t',2),
        ('td',1),('c',0,1),('t',1),('t',0)],
    # What if the S gate gives different phases?
    "SDG_T_TDG": [
        ('h',2),('c',1,2),('sd',2),('c',0,2),('t',2),
        ('c',1,2),('sd',2),('c',0,2),('t',2),
        ('td',1),('c',0,1),('t',1),('t',0),('h',2)],
}

for name, gates in variants.items():
    U = build(gates)
    f = fid(U)
    d = np.max(np.abs(U - _TARGET))
    tc = sum(1 for g in gates if g[0] in ('t','td'))
    print(f"[{name}] F={f:.6f} diff={d:.6f} T-count={tc}")

# Also try: what if the decomposition uses X on q3 instead of H?
print("\n--- With X instead of H on q3 ---")
_X = np.array([[0,1],[1,0]], dtype=complex)
def build_x(gs):
    U = np.eye(8, dtype=complex)
    for g in gs:
        if g[0]=='h': Ug=e1(_X if g[2]=='x' else _H,g[1])
        elif g[0]=='t': Ug=e1(_T,g[1])
        elif g[0]=='td': Ug=e1(_TDG,g[1])
        elif g[0]=='c': Ug=e2(_CNOT,g[1],g[2])
        else: raise ValueError(g[0])
        U = Ug @ U
    return U

# Test if Toffoli can be made differently
print("\n--- Build Toffoli manually ---")
# |110> -> |111>, |111> -> |110>
# Control qubits: q1, q2. Target: q3
# Standard: toffoli = I - |110><110| - |111><111| + |110><111| + |111><110|
# = I + (|110><111| - |110><110| + |111><110| - |111><111|)
# = I + (|11>_c <11|_c) ⊗ (X - I)_t
# where |11>_c = |11> on control qubits, (X-I) on target

manual = np.eye(8, dtype=complex)
# Control pattern: |11> on q1,q2 → states 6 (|110>) and 7 (|111>)
# Add |110><111| (flip q3 when controls=11, q3=1)
manual[6,7] = 1  # |111> -> |110>
# Add |111><110| (flip q3 when controls=11, q3=0)
manual[7,6] = 1  # |110> -> |111>
# Remove identity entries for those states
manual[6,6] = 0  # Remove |110> -> |110>
manual[7,7] = 0  # Remove |111> -> |111>
print(f"Manual Toffoli matches target: {np.allclose(manual, _TARGET)}")
