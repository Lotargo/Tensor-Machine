:- consult('rules.pl').

run_tests :-
    format('=== Test 0: Complex arithmetic ===~n'),
    add_c(c(1,2), c(3,4), Sum),
    format('add_c: ~p~n', [Sum]),
    mul_c(c(0.707,0), c(0.707,0.707), Prod),
    format('mul_c: ~p~n', [Prod]),

    format('~n=== Test 1: Identity ===~n'),
    identity_c(2, I2),
    format('I2: ~p~n', [I2]),
    identity_c(4, I4),
    format('I4: ~p~n', [I4]),

    format('~n=== Test 2: Kron ===~n'),
    identity_c(2, I),
    kron(I, I, I4b),
    format('kron(I,I): ~p~n', [I4b]),

    format('~n=== Test 3: Kron expand ===~n'),
    gate_matrix_c(h(_), H),
    format('H matrix: ~p~n', [H]),
    identity_c(8, I8),
    expand_to_n_qubits(H, [2], 3, U_h3),
    format('H on q3 (3 qubits): ~p~n', [U_h3]),

    format('~n=== Test 4: Gate fidelity ===~n'),
    toffoli_target(T),
    format('Toffoli target 8x8~n'),
    gate_fidelity(T, I8, 8, F_id),
    format('Fidelity identity vs Toffoli: ~f~n', [F_id]),
    gate_fidelity(T, U_h3, 8, F_h3),
    format('Fidelity H(q3) vs Toffoli: ~f~n', [F_h3]),

    format('~n=== Test 5: Build synthesis unitary ===~n'),
    build_synthesis_unitary(h(q1), 3, U_h1),
    format('U for h(q1): ~p~n', [U_h1]),

    format('~n=== All tests complete ===~n').

:- run_tests, halt.
