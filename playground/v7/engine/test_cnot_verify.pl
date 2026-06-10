:- consult('rules.pl').

test :-
    format('=== Verify CNOT expansions ===~n~n'),

    % Expected CNOT(q1,q2) in 3-qubit space (adjacent, control=q1, target=q2)
    % |000⟩→|000⟩, |001⟩→|001⟩, |010⟩→|011⟩, |011⟩→|010⟩,
    % |100⟩→|100⟩, |101⟩→|101⟩, |110⟩→|111⟩, |111⟩→|110⟩
    % This is I_2 ⊗ CNOT = CNOT on q1,q2, identity on q3
    format('Test 1: CNOT(q1,q2) adjacent, 3 qubits~n', []),
    gate_matrix_c(cnot(_,_), CNOT),
    expand_to_n_qubits(CNOT, [0,1], 3, U_cnot_adj),
    print_matrix(U_cnot_adj),

    % Expected CNOT(q2,q3) in 3-qubit space (adjacent, control=q2, target=q3)
    % |000⟩→|000⟩, |001⟩→|000⟩... wait, CNOT on q2,q3:
    % |q1,q2,q3⟩ → |q1,q2,q2⊕q3⟩
    format('~nTest 2: CNOT(q2,q3) adjacent, 3 qubits~n', []),
    expand_to_n_qubits(CNOT, [1,2], 3, U_cnot_q2q3),
    print_matrix(U_cnot_q2q3),

    % Expected CNOT(q1,q3) in 3-qubit space (non-adjacent)
    % |q1,q2,q3⟩ → |q1,q2,q1⊕q3⟩
    % Matrix:
    % [1,0,0,0,0,0,0,0]
    % [0,1,0,0,0,0,0,0]
    % [0,0,1,0,0,0,0,0]
    % [0,0,0,1,0,0,0,0]
    % [0,0,0,0,0,1,0,0]
    % [0,0,0,0,1,0,0,0]
    % [0,0,0,0,0,0,0,1]
    % [0,0,0,0,0,0,1,0]
    format('~nTest 3: CNOT(q1,q3) non-adjacent, 3 qubits~n', []),
    expand_to_n_qubits(CNOT, [0,2], 3, U_cnot_na),
    print_matrix(U_cnot_na),

    format('~nTest 4: Build seq(h(q3), cnot(q1,q3), h(q3))~n', []),
    build_synthesis_unitary(
        seq(h(q3), seq(cnot(q1,q3), h(q3))),
        3, U_seq),
    print_matrix(U_seq),

    format('~n=== Done ===~n', []).

print_matrix([]).
print_matrix([Row|Rows]) :-
    print_row(Row), nl,
    print_matrix(Rows).

print_row([]) :- nl.
print_row([c(Re,_)|Rest]) :-
    format(' ~4f', [Re]),
    print_row(Rest).

:- test, halt.
