:- consult('rules.pl').

test :-
    format('=== Testing Known Toffoli Decomposition ===~n~n'),

    % ===== CORRECT 6-CNOT Toffoli decomposition (Qiskit CCXGate) =====
    % From Qiskit source (x.py: CCXGate._define ASCII comment):
    % Verified: Fidelity=1.0, CNOT=6, T-count=7
    %
    % q1(ctrl1), q2(ctrl2), q3(target):
    % H q3, CNOT(q1,q3), Tdg q3, CNOT(q2,q3), T q3, CNOT(q1,q3),
    % Tdg q3, CNOT(q2,q3), T q3, T q2, CNOT(q1,q2), Tdg q2,
    % CNOT(q1,q2), T q1, H q3
    AST_6CNOT = seq(
        seq(seq(seq(seq(seq(seq(seq(seq(seq(seq(seq(seq(seq(
            h(q3),
            cnot(q1,q3)),
            tdg(q3)),
            cnot(q2,q3)),
            t(q3)),
            cnot(q1,q3)),
            tdg(q3)),
            cnot(q2,q3)),
            t(q3)),
            t(q2)),
            cnot(q1,q2)),
            tdg(q2)),
            cnot(q1,q2)),
            t(q1)),
        h(q3)),

    censor(target, synthesis, AST_6CNOT, Penalty_6CNOT),
    format('6-CNOT Toffoli: Penalty = ~f~n', [Penalty_6CNOT]),

    % Also test: 4-CNOT decomposition (INCORRECT - gives cyclic permutation)
    % This is the "textbook" version from Nielsen & Chuang that gives Fidelity=0.5
    AST_4CNOT = seq(
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
    censor(target, synthesis, AST_4CNOT, Penalty_4CNOT),
    format('4-CNOT Toffoli (textbook, INCORRECT): Penalty = ~f~n', [Penalty_4CNOT]),

    % Also test a fragment: just the H gates
    censor(target, synthesis, seq(seq(h(q1), h(q2)), h(q3)), P_3h),
    format('HHH (all 3 H gates): Penalty = ~f~n', [P_3h]),

    format('~n=== Done ===~n').

:- test, halt.
