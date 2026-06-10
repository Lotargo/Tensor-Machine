"""Debug gate ordering and basic identities."""
import numpy as np

_I = np.eye(2, dtype=complex)
_H = np.array([[1, 1], [1, -1]], dtype=complex) / np.sqrt(2)
_T = np.array([[1, 0], [0, np.exp(1j * np.pi / 4)]], dtype=complex)
_TDG = np.array([[1, 0], [0, np.exp(-1j * np.pi / 4)]], dtype=complex)
_CNOT = np.array([[1, 0, 0, 0],
                  [0, 1, 0, 0],
                  [0, 0, 0, 1],
                  [0, 0, 1, 0]], dtype=complex)

def kron_n(ms):
    r = ms[0]
    for m in ms[1:]: r = np.kron(r, m)
    return r

def e1(g, q, n=3):
    ms = [_I]*n; ms[q]=g; return kron_n(ms)

def e2v1(g, c, t, n=3):
    """Version 1: original (c=row, t=col within row)"""
    r = np.zeros((2**n,2**n), dtype=complex)
    for i in range(2**n):
        for j in range(2**n):
            bic=(i>>(n-c-1))&1; bjc=(j>>(n-c-1))&1
            bit=(i>>(n-t-1))&1; bjt=(j>>(n-t-1))&1
            is_ctrl=(bic<<1)|bit
            js_ctrl=(bjc<<1)|bjt
            ok=all(((i>>(n-k-1))&1)==((j>>(n-k-1))&1)
                   for k in range(n) if k not in (c,t))
            if ok: r[i,j]=g[is_ctrl,js_ctrl]
    return r

def e2v2(g, c, t, n=3):
    """Version 2: target=row, control=col within row"""
    r = np.zeros((2**n,2**n), dtype=complex)
    for i in range(2**n):
        for j in range(2**n):
            bic=(i>>(n-c-1))&1; bjc=(j>>(n-c-1))&1
            bit=(i>>(n-t-1))&1; bjt=(j>>(n-t-1))&1
            is_tgt=(bit<<1)|bic
            js_tgt=(bjt<<1)|bjc
            ok=all(((i>>(n-k-1))&1)==((j>>(n-k-1))&1)
                   for k in range(n) if k not in (c,t))
            if ok: r[i,j]=g[is_tgt,js_tgt]
    return r

# Verify basic CNOT on 2 qubits
print("=== CNOT verification on 2 qubits ===")
for name, e2f in [("v1", e2v1), ("v2", e2v2)]:
    cnot_2q = e2f(_CNOT, 0, 1, 2)
    correct = True
    for i in range(4):
        v = np.zeros(4); v[i] = 1
        out = cnot_2q @ v
        j = np.argmax(np.abs(out))
        # Expected: |00>->|00>, |01>->|01>, |10>->|11>, |11>->|10>
        expected = i if i < 2 else (3 if i == 2 else 2)
        ok = (j == expected)
        if not ok: correct = False
        print(f"  {name} |{i:02b}> -> |{j:02b}> {'OK' if ok else 'FAIL expected |{expected:02b}>'}")
    print(f"  {name} all correct: {correct}")

# Build Toffoli with both versions
_TOFF = np.eye(8, dtype=complex)
_TOFF[6,6]=0; _TOFF[6,7]=1; _TOFF[7,6]=1; _TOFF[7,7]=0

def fid(U, target=_TOFF):
    p = target.conj().T @ U; return np.abs(np.trace(p))/8

def build(gs, e2f):
    U = np.eye(8, dtype=complex)
    for g in gs:
        if g[0]=='h': Ug=e1(_H,g[1])
        elif g[0]=='t': Ug=e1(_T,g[1])
        elif g[0]=='td': Ug=e1(_TDG,g[1])
        elif g[0]=='c': Ug=e2f(_CNOT,g[1],g[2])
        U = Ug @ U
    return U

maslov = [('h',2),('c',1,2),('td',2),('c',0,2),('t',2),
          ('c',1,2),('td',2),('c',0,2),('t',2),
          ('td',1),('c',0,1),('t',1),('t',0),('h',2)]

for name, e2f in [("v1", e2v1), ("v2", e2v2)]:
    U = build(maslov, e2f)
    f = fid(U)
    d = np.max(np.abs(U - _TOFF))
    print(f"\nToffoli with {name}: F={f:.6f} diff={d:.6f}")
    for inp, desc in [(6,"|110>"),(7,"|111>"),(4,"|100>"),(5,"|101>")]:
        v=np.zeros(8); v[inp]=1
        out=U@v; j=np.argmax(np.abs(out))
        print(f"  {desc} -> |{j:03b}>")

# Try left-multiply vs right-multiply
print("\n=== Gate order: left-multiply vs right-multiply ===")
for mul_order, mul_desc in [(lambda Ug,U: Ug@U, "Ug @ U (left-mul)"),
                              (lambda Ug,U: U@Ug, "U @ Ug (right-mul)")]:
    U = np.eye(8, dtype=complex)
    for g in maslov:
        if g[0]=='h': Ug=e1(_H,g[1])
        elif g[0]=='t': Ug=e1(_T,g[1])
        elif g[0]=='td': Ug=e1(_TDG,g[1])
        elif g[0]=='c': Ug=e2v1(_CNOT,g[1],g[2])
        U = mul_order(Ug, U)
    f = fid(U)
    d = np.max(np.abs(U - _TOFF))
    print(f"  {mul_desc}: F={f:.6f} diff={d:.6f}")
