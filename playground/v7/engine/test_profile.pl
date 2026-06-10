:- consult('rules.pl').

test :-
    format('=== Profiling Synthesis ===~n', []),
    flush_output,

    % Step 1: Load target
    format('Step 1: Loading Toffoli target...~n', []), flush_output,
    toffoli_target(T),
    format('  Done. Target is ~d x ~d matrix~n', [8, 8]), flush_output,

    % Step 2: Single gate
    format('Step 2: Building h(q3) unitary...~n', []), flush_output,
    time(build_synthesis_unitary(h(q3), 3, U_h3q)),
    format('  Done.~n', []), flush_output,

    % Step 3: Gate fidelity
    format('Step 3: Computing fidelity h(q3) vs Toffoli...~n', []), flush_output,
    time(gate_fidelity(T, U_h3q, 8, F1)),
    format('  Fidelity h(q3) vs Toffoli: ~f~n', [F1]), flush_output,

    % Step 4: Simple seq of two gates
    format('Step 4: Building seq(h(q1), h(q2))...~n', []), flush_output,
    time(build_synthesis_unitary(seq(h(q1), h(q2)), 3, U_hh)),
    format('  Done.~n', []), flush_output,
    time(gate_fidelity(T, U_hh, 8, F2)),
    format('  Fidelity h(q1),h(q2) vs Toffoli: ~f~n', [F2]), flush_output,

    % Step 5: Three H gates
    format('Step 5: Building seq(seq(h(q1), h(q2)), h(q3))...~n', []), flush_output,
    time(build_synthesis_unitary(seq(seq(h(q1), h(q2)), h(q3)), 3, U_3h)),
    format('  Done.~n', []), flush_output,
    time(gate_fidelity(T, U_3h, 8, F3)),
    format('  Fidelity 3xH vs Toffoli: ~f~n', [F3]), flush_output,

    % Step 6: Single CNOT
    format('Step 6: Building cnot(q1,q2) unitary...~n', []), flush_output,
    format('Step 6a: gate_matrix_c...~n', []), flush_output,
    time(gate_matrix_c(cnot(_A,_B), G)),
    format('  Gate matrix: ~p~n', [G]), flush_output,
    format('Step 6b: expand_to_n_qubits...~n', []), flush_output,
    time(expand_to_n_qubits(G, [1,2], 3, U_cnot)),
    format('  Done.~n', []), flush_output,

    format('Step 7: Testing censor dispatch...~n', []), flush_output,
    time(censor(target, synthesis, seq(h(q1), h(q2)), P)),
    format('  censor penalty: ~f~n', [P]), flush_output,

    format('~n=== Profiling Done ===~n', []), flush_output.

:- test, halt.
