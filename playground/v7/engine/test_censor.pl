:- consult('rules.pl').

test :-
    format('Test 1: synthesis_censor with 3 qubits~n', []),
    (   synthesis_censor(seq(seq(h(q1), h(q2)), h(q3)), P1)
    ->  format('  OK: Penalty = ~f~n', [P1])
    ;   format('  FAIL~n', [])
    ),

    format('Test 2: censor dispatch with 3 qubits~n', []),
    (   censor(target, synthesis, seq(seq(h(q1), h(q2)), h(q3)), P2)
    ->  format('  OK: Penalty = ~f~n', [P2])
    ;   format('  FAIL~n', [])
    ),

    format('Test 3: censor with known Toffoli (N&C, ~p)~n', ['T-count=7']),
    AST = seq(
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
    (   censor(target, synthesis, AST, P3)
    ->  format('  OK: Penalty = ~f~n', [P3])
    ;   format('  FAIL~n', [])
    ),

    format('Done.~n', []).

:- test, halt.
