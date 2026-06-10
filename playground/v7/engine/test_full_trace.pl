:- consult('rules.pl').

test :-
    AST = seq(
        seq(seq(seq(seq(seq(seq(seq(seq(seq(seq(seq(
            h(q3), tdg(q3)),
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

    format('Test 1: valid_quantum_structure~n', []),
    (   valid_quantum_structure(AST)
    ->  format('  OK~n', [])
    ;   format('  FAIL~n', [])
    ),

    format('Test 2: find_qubits~n', []),
    find_qubits(AST, Qs),
    format('  ~p~n', [Qs]),

    format('Test 3: build_synthesis_unitary~n', []),
    (   build_synthesis_unitary(AST, 3, U)
    ->  format('  OK, matrix ~d x ~d~n', [8, 8])
    ;   format('  FAIL~n', [])
    ),

    format('Test 4: toffoli_target~n', []),
    toffoli_target(T),
    format('  OK, target loaded~n', []),

    format('Test 5: gate_fidelity~n', []),
    (   build_synthesis_unitary(AST, 3, U2),
        gate_fidelity(T, U2, 8, F)
    ->  format('  Fidelity = ~f~n', [F])
    ;   format('  FAIL~n', [])
    ),

    format('Done.~n', []).

:- test, halt.
