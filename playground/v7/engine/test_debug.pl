:- consult('rules.pl').

% Helper: true identity on 3 qubits: H·H on each qubit = I
identity_3q(seq(seq(seq(h(q1), h(q1)), seq(h(q2), h(q2))), seq(h(q3), h(q3)))).

test :-
    format('=== Debug: Synthesis Censor Tests (all 3-qubit) ===~n~n'),
    toffoli_target(TTarget),

    % Test 1: Pure identity
    format('--- Test 1: Pure identity (H·H on each qubit) ---~n'),
    identity_3q(AST1),
    build_synthesis_unitary(AST1, 3, U1),
    gate_fidelity(TTarget, U1, 8, F1),
    ctrace(U1, c(R1,I1)), T1 is sqrt(R1*R1 + I1*I1),
    censor(target, synthesis, AST1, P1),
    format('  Fidelity=~f, Trace=~f, Penalty=~f~n', [F1, T1, P1]),

    % Test 2: HHH
    format('~n--- Test 2: HHH ---~n'),
    AST2 = seq(seq(h(q1), h(q2)), h(q3)),
    build_synthesis_unitary(AST2, 3, U2),
    gate_fidelity(TTarget, U2, 8, F2),
    ctrace(U2, c(R2,I2)), T2 is sqrt(R2*R2 + I2*I2),
    censor(target, synthesis, AST2, P2),
    format('  Fidelity=~f, Trace=~f, Penalty=~f~n', [F2, T2, P2]),

    % Test 3: CNOT(q1,q2)
    format('~n--- Test 3: CNOT(q1,q2) on 3 qubits ---~n'),
    identity_3q(Id3),
    AST3 = seq(Id3, cnot(q1,q2)),
    build_synthesis_unitary(AST3, 3, U3),
    gate_fidelity(TTarget, U3, 8, F3),
    ctrace(U3, c(R3,I3)), T3 is sqrt(R3*R3 + I3*I3),
    censor(target, synthesis, AST3, P3),
    format('  Fidelity=~f, Trace=~f, Penalty=~f~n', [F3, T3, P3]),

    % Test 4: CNOT(q1,q3) (non-adjacent)
    format('~n--- Test 4: CNOT(q1,q3) (non-adjacent) on 3 qubits ---~n'),
    AST4 = seq(Id3, cnot(q1,q3)),
    build_synthesis_unitary(AST4, 3, U4),
    gate_fidelity(TTarget, U4, 8, F4),
    ctrace(U4, c(R4,I4)), T4 is sqrt(R4*R4 + I4*I4),
    censor(target, synthesis, AST4, P4),
    format('  Fidelity=~f, Trace=~f, Penalty=~f~n', [F4, T4, P4]),

    % Test 5: Known Toffoli
    format('~n--- Test 5: Known Toffoli ---~n'),
    AST5 = seq(
        seq(seq(seq(seq(seq(seq(seq(seq(seq(seq(seq(
            h(q3),
            tdg(q3)),
            cnot(q2,q3)),
            t(q3)),
            cnot(q1,q3)),
            tdg(q3)),
            cnot(q2,q3)),
            t(q3)),
            tdg(q2)),
            cnot(q1,q2)),
            t(q2)),
            t(q1)),
        h(q3)),
    build_synthesis_unitary(AST5, 3, U5),
    gate_fidelity(TTarget, U5, 8, F5),
    ctrace(U5, c(R5,I5)), T5 is sqrt(R5*R5 + I5*I5),
    censor(target, synthesis, AST5, P5),
    format('  Fidelity=~f, Trace=~f, Penalty=~f~n', [F5, T5, P5]),

    % Test 6: Identity + CNOT chain (should have fidelity 0.75, trace dependent)
    format('~n--- Test 6: CNOT chain (should be fidelity 0.75 range) ---~n'),
    AST6 = seq(Id3, seq(cnot(q1,q2), cnot(q2,q3))),
    build_synthesis_unitary(AST6, 3, U6),
    gate_fidelity(TTarget, U6, 8, F6),
    ctrace(U6, c(R6,I6)), T6 is sqrt(R6*R6 + I6*I6),
    censor(target, synthesis, AST6, P6),
    format('  CNOT*2: Fidelity=~f, Trace=~f, Penalty=~f~n', [F6, T6, P6]),

    format('~n========================================================~n'),
    format('SUMMARY:~n'),
    format('  Identity:            F=~6f  Tr=~6f  P=~6f~n', [F1, T1, P1]),
    format('  HHH:                 F=~6f  Tr=~6f  P=~6f~n', [F2, T2, P2]),
    format('  CNOT(q1,q2):        F=~6f  Tr=~6f  P=~6f~n', [F3, T3, P3]),
    format('  CNOT(q1,q3):        F=~6f  Tr=~6f  P=~6f~n', [F4, T4, P4]),
    format('  Known Toffoli:      F=~6f  Tr=~6f  P=~6f~n', [F5, T5, P5]),
    format('  CNOT chain:         F=~6f  Tr=~6f  P=~6f~n', [F6, T6, P6]),
    format('========================================================~n'),
    format('=== Done ===~n').

:- test, halt.
