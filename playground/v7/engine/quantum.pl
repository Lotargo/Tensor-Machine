% quantum.pl - Prolog Quantum Physics invariants & Heuristic Penalty Quantum Simulator

:- dynamic lambda_synthesis/1.
lambda_synthesis(0.01).

quantum_censor(AST, Penalty) :-
    (   \+ valid_quantum_structure(AST)
    ->  Penalty = 1000.0
    ;   (   \+ is_unitary_ast(AST)
        ->  Penalty = 1000.0
        ;   (   is_quantum_redundant(AST)
            ->  Penalty = 900.0
            ;   find_qubits(AST, RawQubits),
                (   RawQubits = []
                ->  Qubits = [q1, q2]
                ;   (   length(RawQubits, 1)
                    ->  union(RawQubits, [q2], Qubits)
                    ;   Qubits = RawQubits
                    )
                ),
                sort_qubits(Qubits, SortedQs),
                length(SortedQs, N),
                create_ground_state(N, GroundVec),
                % Initial state for Grover's should be equal superposition,
                % but standard practice is starting from |00> and engine must apply H gates itself
                GroundState = [amp(c(1, 0), GroundVec)],
                (   catch(eval_quantum(AST, SortedQs, GroundState, FinalState), _, fail)
                ->  quantum_fidelity(FinalState, SortedQs, Fidelity),
                    BasePenalty is 1000.0 * (1.0 - Fidelity),
                    % Occam's Razor: If fidelity is very high (> 0.99), apply minor penalty for AST size
                    (   Fidelity > 0.99
                    ->  ast_size(AST, Size),
                        Penalty is BasePenalty + (Size * 0.01)
                    ;   Penalty = BasePenalty
                    )
                ;   Penalty = 1000.0
                )
            )
        )
    ).

% Simple AST node counter
ast_size(AST, Size) :-
    (   atomic(AST)
    ->  Size = 1
    ;   compound(AST)
    ->  AST =.. [_|Args],
        map_ast_size(Args, ArgSizes),
        sum_list(ArgSizes, Sum),
        Size is Sum + 1
    ).

map_ast_size([], []).
map_ast_size([H|T], [S|ST]) :-
    ast_size(H, S),
    map_ast_size(T, ST).

% Qubit name format is q followed by integer
valid_quantum_structure(X) :- atom(X), atom_concat(q, _, X), !.

% Quantum Gates structure
valid_quantum_structure(h(X)) :- valid_quantum_structure(X).
valid_quantum_structure(x_gate(X)) :- valid_quantum_structure(X).
valid_quantum_structure(y_gate(X)) :- valid_quantum_structure(X).
valid_quantum_structure(z_gate(X)) :- valid_quantum_structure(X).
valid_quantum_structure(t(X)) :- valid_quantum_structure(X).
valid_quantum_structure(tdg(X)) :- valid_quantum_structure(X).
valid_quantum_structure(s(X)) :- valid_quantum_structure(X).
valid_quantum_structure(sdg(X)) :- valid_quantum_structure(X).
valid_quantum_structure(cnot(Control, Target)) :-
    valid_quantum_structure(Control),
    valid_quantum_structure(Target),
    Control \= Target.
valid_quantum_structure(oracle(Control, Target)) :-
    valid_quantum_structure(Control),
    valid_quantum_structure(Target),
    Control \= Target.

% Sequences and tensors
valid_quantum_structure(seq(A, B)) :-
    valid_quantum_structure(A),
    valid_quantum_structure(B).
valid_quantum_structure(tensor(A, B)) :-
    valid_quantum_structure(A),
    valid_quantum_structure(B).

% Occam's Razor: prune physical self-inverse gate redundancies
is_quantum_redundant(h(h(_))).
is_quantum_redundant(x_gate(x_gate(_))).
is_quantum_redundant(y_gate(y_gate(_))).
is_quantum_redundant(z_gate(z_gate(_))).
is_quantum_redundant(t(tdg(_))).
is_quantum_redundant(tdg(t(_))).
is_quantum_redundant(s(sdg(_))).
is_quantum_redundant(sdg(s(_))).
is_quantum_redundant(seq(A, B)) :- A == B, is_self_inverse(A).

% Propagate redundancy checks
is_quantum_redundant(h(X)) :- is_quantum_redundant(X).
is_quantum_redundant(x_gate(X)) :- is_quantum_redundant(X).
is_quantum_redundant(y_gate(X)) :- is_quantum_redundant(X).
is_quantum_redundant(z_gate(X)) :- is_quantum_redundant(X).
is_quantum_redundant(t(X)) :- is_quantum_redundant(X).
is_quantum_redundant(tdg(X)) :- is_quantum_redundant(X).
is_quantum_redundant(s(X)) :- is_quantum_redundant(X).
is_quantum_redundant(sdg(X)) :- is_quantum_redundant(X).
is_quantum_redundant(cnot(C, _)) :- is_quantum_redundant(C).
is_quantum_redundant(cnot(_, T)) :- is_quantum_redundant(T).
is_quantum_redundant(oracle(Q1, _)) :- is_quantum_redundant(Q1).
is_quantum_redundant(oracle(_, Q2)) :- is_quantum_redundant(Q2).
is_quantum_redundant(seq(A, _)) :- is_quantum_redundant(A).
is_quantum_redundant(seq(_, B)) :- is_quantum_redundant(B).
is_quantum_redundant(tensor(A, _)) :- is_quantum_redundant(A).
is_quantum_redundant(tensor(_, B)) :- is_quantum_redundant(B).

is_self_inverse(h(_)).
is_self_inverse(x_gate(_)).
is_self_inverse(y_gate(_)).
is_self_inverse(z_gate(_)).
is_self_inverse(oracle(_, _)).

% =======================================================================
% Dynamic Qubit List Extractors & Sorters
% =======================================================================

find_qubits(Q, [Q]) :- atom(Q), atom_concat(q, _, Q), !.
find_qubits(h(Q), Qs) :- !, find_qubits(Q, Qs).
find_qubits(x_gate(Q), Qs) :- !, find_qubits(Q, Qs).
find_qubits(y_gate(Q), Qs) :- !, find_qubits(Q, Qs).
find_qubits(z_gate(Q), Qs) :- !, find_qubits(Q, Qs).
find_qubits(t(Q), Qs) :- !, find_qubits(Q, Qs).
find_qubits(tdg(Q), Qs) :- !, find_qubits(Q, Qs).
find_qubits(s(Q), Qs) :- !, find_qubits(Q, Qs).
find_qubits(sdg(Q), Qs) :- !, find_qubits(Q, Qs).
find_qubits(cnot(C, T), Qs) :-
    !,
    find_qubits(C, QsC),
    find_qubits(T, QsT),
    union(QsC, QsT, Qs).
find_qubits(oracle(Q1, Q2), Qs) :-
    !,
    find_qubits(Q1, Qs1),
    find_qubits(Q2, Qs2),
    union(Qs1, Qs2, Qs).
find_qubits(seq(A, B), Qs) :-
    !,
    find_qubits(A, QsA),
    find_qubits(B, QsB),
    union(QsA, QsB, Qs).
find_qubits(tensor(A, B), Qs) :-
    !,
    find_qubits(A, QsA),
    find_qubits(B, QsB),
    union(QsA, QsB, Qs).
find_qubits(_, []).

sort_qubits(Qs, Sorted) :-
    predsort(compare_qubits, Qs, Sorted).

compare_qubits(Delta, Q1, Q2) :-
    (   atom_concat(q, NumStr1, Q1),
        atom_concat(q, NumStr2, Q2),
        atom_number(NumStr1, N1),
        atom_number(NumStr2, N2)
    ->  compare(Delta, N1, N2)
    ;   compare(Delta, Q1, Q2)
    ).

qubit_position(Q, [Q|_], 0) :- !.
qubit_position(Q, [_|T], Index) :-
    qubit_position(Q, T, Index1),
    Index is Index1 + 1.

create_ground_state(0, []) :- !.
create_ground_state(N, [0|T]) :-
    N > 0,
    N1 is N - 1,
    create_ground_state(N1, T).

% =======================================================================
% Symbolic Hilbert Space Simulator
% =======================================================================

get_bit([B|_], 0, B) :- !.
get_bit([_|T], I, B) :- I > 0, I1 is I - 1, get_bit(T, I1, B).

set_bit([_|T], 0, Val, [Val|T]) :- !.
set_bit([H|T], I, Val, [H|R]) :- I > 0, I1 is I - 1, set_bit(T, I1, Val, R).

simplify_state([], []).
simplify_state([H|T], Out) :-
    simplify_state(T, Mid),
    add_to_state(H, Mid, Out).

add_to_state(amp(c(N, R), Vec), [], [amp(c(N, R), Vec)]) :- !.
add_to_state(amp(c(N1, R), Vec), [amp(c(N2, R), Vec)|T], Out) :-
    !,
    N3 is N1 + N2,
    (   N3 =:= 0
    ->  Out = T
    ;   Out = [amp(c(N3, R), Vec)|T]
    ).
add_to_state(amp(c(N1, R1), Vec), [amp(c(N2, R2), Vec)|T], Out) :-
    !,
    MaxR is max(R1, R2),
    scale_amp(c(N1, R1), MaxR, c(Scal1, MaxR)),
    scale_amp(c(N2, R2), MaxR, c(Scal2, MaxR)),
    N3 is Scal1 + Scal2,
    (   N3 =:= 0
    ->  Out = T
    ;   Out = [amp(c(N3, MaxR), Vec)|T]
    ).
add_to_state(Term, [H|T], [H|R]) :-
    add_to_state(Term, T, R).

scale_amp(c(N, R), TargetR, c(Scal, TargetR)) :-
    Diff is TargetR - R,
    (   Diff =:= 0
    ->  Scal = N
    ;   (   Diff mod 2 =:= 0
        ->  Power is Diff // 2,
            Scal is N * (2 ** Power)
        ;   Scal = N
        )
    ).

% Gate Applications
apply_h_to_state(Index, InState, OutState) :-
    apply_h_loop(InState, Index, RawState),
    simplify_state(RawState, OutState).

apply_h_loop([], _, []).
apply_h_loop([amp(c(N, R), Vec)|T], Index, [amp(c(N, R1), Vec0), amp(c(SignN, R1), Vec1)|Rrest]) :-
    R1 is R + 1,
    get_bit(Vec, Index, Bit),
    set_bit(Vec, Index, 0, Vec0),
    set_bit(Vec, Index, 1, Vec1),
    (   Bit =:= 0
    ->  SignN = N
    ;   SignN is -N
    ),
    apply_h_loop(T, Index, Rrest).

apply_x_to_state(Index, InState, OutState) :-
    apply_x_loop(InState, Index, RawState),
    simplify_state(RawState, OutState).

apply_x_loop([], _, []).
apply_x_loop([amp(Coeff, Vec)|T], Index, [amp(Coeff, NewVec)|Rrest]) :-
    get_bit(Vec, Index, Bit),
    (   Bit =:= 0
    ->  NewBit = 1
    ;   NewBit = 0
    ),
    set_bit(Vec, Index, NewBit, NewVec),
    apply_x_loop(T, Index, Rrest).

apply_cnot_to_state(IndexC, IndexT, InState, OutState) :-
    apply_cnot_loop(InState, IndexC, IndexT, RawState),
    simplify_state(RawState, OutState).

apply_cnot_loop([], _, _, []).
apply_cnot_loop([amp(Coeff, Vec)|T], IndexC, IndexT, [amp(Coeff, NewVec)|Rrest]) :-
    get_bit(Vec, IndexC, BitC),
    (   BitC =:= 1
    ->  get_bit(Vec, IndexT, BitT),
        (   BitT =:= 0
        ->  NewBitT = 1
        ;   NewBitT = 0
        ),
        set_bit(Vec, IndexT, NewBitT, NewVec)
    ;   NewVec = Vec
    ),
    apply_cnot_loop(T, IndexC, IndexT, Rrest).

apply_oracle_to_state(Index1, Index2, InState, OutState) :-
    apply_oracle_loop(InState, Index1, Index2, RawState),
    simplify_state(RawState, OutState).

apply_oracle_loop([], _, _, []).
apply_oracle_loop([amp(c(N, R), Vec)|T], Index1, Index2, [amp(c(SignN, R), Vec)|Rrest]) :-
    get_bit(Vec, Index1, Bit1),
    get_bit(Vec, Index2, Bit2),
    (   (Bit1 =:= 1, Bit2 =:= 1) % Target state is |11>
    ->  SignN is -N
    ;   SignN = N
    ),
    apply_oracle_loop(T, Index1, Index2, Rrest).

% Phase gate: multiply amplitude by Phase when qubit is |1>
apply_phase_to_state(Index, Phase, InState, OutState) :-
    apply_phase_loop(InState, Index, Phase, RawState),
    simplify_state(RawState, OutState).

apply_phase_loop([], _, _, []).
apply_phase_loop([amp(Coeff, Vec)|T], Index, Phase, [amp(NewCoeff, Vec)|Rrest]) :-
    get_bit(Vec, Index, Bit),
    (   Bit =:= 1
    ->  mul_c(Coeff, Phase, NewCoeff)
    ;   NewCoeff = Coeff
    ),
    apply_phase_loop(T, Index, Phase, Rrest).

% Y gate: flip qubit with phase (Y|0> = i|1>, Y|1> = -i|0>)
apply_y_to_state(Index, InState, OutState) :-
    apply_y_loop(InState, Index, RawState),
    simplify_state(RawState, OutState).

apply_y_loop([], _, []).
apply_y_loop([amp(Coeff, Vec)|T], Index, [amp(NewCoeff, NewVec)|Rrest]) :-
    get_bit(Vec, Index, Bit),
    (   Bit =:= 0
    ->  set_bit(Vec, Index, 1, NewVec),
        mul_c(Coeff, c(0,1), NewCoeff)
    ;   set_bit(Vec, Index, 0, NewVec),
        mul_c(Coeff, c(0,-1), NewCoeff)
    ),
    apply_y_loop(T, Index, Rrest).

% eval_quantum(AST, SortedQs, InState, OutState)
eval_quantum(X, _, State, State) :- atom(X), !.

eval_quantum(h(Q), SortedQs, InState, OutState) :-
    !,
    qubit_position(Q, SortedQs, Index),
    apply_h_to_state(Index, InState, OutState).

eval_quantum(t(Q), SortedQs, InState, OutState) :-
    !,
    qubit_position(Q, SortedQs, Index),
    apply_phase_to_state(Index, c(0.70710678, 0.70710678), InState, OutState).

eval_quantum(tdg(Q), SortedQs, InState, OutState) :-
    !,
    qubit_position(Q, SortedQs, Index),
    apply_phase_to_state(Index, c(0.70710678, -0.70710678), InState, OutState).

eval_quantum(s(Q), SortedQs, InState, OutState) :-
    !,
    qubit_position(Q, SortedQs, Index),
    apply_phase_to_state(Index, c(0, 1), InState, OutState).

eval_quantum(sdg(Q), SortedQs, InState, OutState) :-
    !,
    qubit_position(Q, SortedQs, Index),
    apply_phase_to_state(Index, c(0, -1), InState, OutState).

eval_quantum(y_gate(Q), SortedQs, InState, OutState) :-
    !,
    qubit_position(Q, SortedQs, Index),
    apply_y_to_state(Index, InState, OutState).

eval_quantum(z_gate(Q), SortedQs, InState, OutState) :-
    !,
    qubit_position(Q, SortedQs, Index),
    apply_phase_to_state(Index, c(-1, 0), InState, OutState).

eval_quantum(x_gate(Q), SortedQs, InState, OutState) :-
    !,
    qubit_position(Q, SortedQs, Index),
    apply_x_to_state(Index, InState, OutState).

eval_quantum(cnot(C, T), SortedQs, InState, OutState) :-
    !,
    qubit_position(C, SortedQs, IndexC),
    qubit_position(T, SortedQs, IndexT),
    apply_cnot_to_state(IndexC, IndexT, InState, OutState).

eval_quantum(oracle(Q1, Q2), SortedQs, InState, OutState) :-
    !,
    qubit_position(Q1, SortedQs, Index1),
    qubit_position(Q2, SortedQs, Index2),
    apply_oracle_to_state(Index1, Index2, InState, OutState).

eval_quantum(seq(A, B), SortedQs, InState, OutState) :-
    !,
    eval_quantum(A, SortedQs, InState, MidState),
    eval_quantum(B, SortedQs, MidState, OutState).

eval_quantum(tensor(A, B), SortedQs, InState, OutState) :-
    !,
    eval_quantum(A, SortedQs, InState, MidState),
    eval_quantum(B, SortedQs, MidState, OutState).

% =======================================================================
% Continuous Quantum Fidelity Gradients
% =======================================================================

coeff_to_float(c(N, R), Val) :-
    Val is N / (sqrt(2) ** R).

get_state_amplitude(Vec, State, Amp) :-
    (   member(amp(Coeff, Vec), State)
    ->  coeff_to_float(Coeff, Amp)
    ;   Amp = 0.0
    ).

quantum_fidelity(State, SortedQs, Fidelity) :-
    length(SortedQs, N),
    (   N < 2
    ->  Fidelity = 0.0
    ;   % Grover's Algorithm goal: Maximize amplitude of |11...1> state
        create_grover_target(N, TargetVec),
        get_state_amplitude(TargetVec, State, Amp),
        Fidelity is Amp * Amp % Probability of measuring target state
    ).

create_grover_target(0, []) :- !.
create_grover_target(N, [1|T]) :-
    N > 0,
    N1 is N - 1,
    create_grover_target(N1, T).

all_zeros([]).
all_zeros([0|T]) :- all_zeros(T).


% =======================================================================
% Semantic Unitary Verification (Algebraic Matrices)
% =======================================================================

% Map Gates to Real Matrices
gate_matrix(h(_), [[0.70710678, 0.70710678], [0.70710678, -0.70710678]]).
gate_matrix(x_gate(_), [[0.0, 1.0], [1.0, 0.0]]).
gate_matrix(y_gate(_), [[0.0, -1.0], [1.0, 0.0]]). % Assuming Real representation for PoC (iY)
gate_matrix(z_gate(_), [[1.0, 0.0], [0.0, -1.0]]).
gate_matrix(cnot(_, _), [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 0.0]]).
gate_matrix(oracle(_, _), [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, -1.0]]). % Flips sign of |11>

build_unitary(X, M) :- gate_matrix(X, M), !.
build_unitary(seq(A, B), M) :-
    build_unitary(A, M_A),
    build_unitary(B, M_B),
    length(M_A, L_A),
    length(M_B, L_B),
    (   L_A =:= L_B
    ->  matrix_multiply(M_B, M_A, M) % For sequential gates, matrices must be same size
    ;   M = [[1.0, 0.0], [0.0, 1.0]] % Mismatch size fallback
    ).
build_unitary(tensor(_, _), [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]). % Fallback identity 4x4
build_unitary(_, [[1.0, 0.0], [0.0, 1.0]]). % Fallback identity 2x2

% Vector dot product
dot_product([], [], 0.0).
dot_product([H1|T1], [H2|T2], Result) :-
    dot_product(T1, T2, Rest),
    Result is H1 * H2 + Rest.

% Get column from matrix
get_column([], _, []).
get_column([Row|Matrix], ColIndex, [Elem|ColRest]) :-
    nth0(ColIndex, Row, Elem),
    get_column(Matrix, ColIndex, ColRest).

% Matrix multiplication
matrix_multiply(M1, M2, Result) :-
    M2 = [Row1|_],
    length(Row1, NumCols),
    numlist(0, NumCols, ColIndicesExt),
    append(ColIndices, [_], ColIndicesExt),
    maplist(multiply_row(M2, ColIndices), M1, Result).

multiply_row(M2, ColIndices, Row, ResultRow) :-
    maplist(multiply_cell(M2, Row), ColIndices, ResultRow).

multiply_cell(M2, Row, ColIndex, Result) :-
    get_column(M2, ColIndex, Col),
    dot_product(Row, Col, Result).

% Matrix transpose (Conjugate Transpose for Real matrices)
transpose([[]|_], []).
transpose(Matrix, [Col|T]) :-
    matrix_first_col(Matrix, Col, RestMatrix),
    transpose(RestMatrix, T).

matrix_first_col([], [], []).
matrix_first_col([[H|T]|Rows], [H|Hs], [T|Ts]) :-
    matrix_first_col(Rows, Hs, Ts).

% Round matrix elements to handle float inaccuracies
round_matrix([], []).
round_matrix([Row|T], [RRow|RT]) :-
    maplist(round_elem, Row, RRow),
    round_matrix(T, RT).

round_elem(E, RE) :- RE is round(E * 1000000.0) / 1000000.0.

% Check Identity matrix
is_identity_matrix(M) :-
    length(M, N),
    numlist(0, N, IndicesExt),
    append(Indices, [_], IndicesExt),
    maplist(check_identity_row(N), Indices, M).

check_identity_row(N, RowIndex, Row) :-
    length(Row, N),
    nth0(RowIndex, Row, 1.0),
    sum_list(Row, 1.0).

% Full Unitary Check (U * U^T = I)
is_unitary_ast(AST) :-
    build_unitary(AST, U),
    transpose(U, UT),
    matrix_multiply(U, UT, Product),
    round_matrix(Product, RoundedProduct),
    is_identity_matrix(RoundedProduct).

% =======================================================================
% Quantum Synthesis: Complex Matrix Arithmetic & Gate Fidelity
% =======================================================================

% Complex number: c(Re, Im)
add_c(c(R1,I1), c(R2,I2), c(R3,I3)) :- R3 is R1+R2, I3 is I1+I2.
sub_c(c(R1,I1), c(R2,I2), c(R3,I3)) :- R3 is R1-R2, I3 is I1-I2.
mul_c(c(R1,I1), c(R2,I2), c(R3,I3)) :- R3 is R1*R2 - I1*I2, I3 is R1*I2 + I1*R2.
conj_c(c(R,I), c(R,NI)) :- NI is -I.
mag2_c(c(R,I), M2) :- M2 is R*R + I*I.
mag_c(c(R,I), M) :- M is sqrt(R*R + I*I).

% Complex gate matrices for synthesis
gate_matrix_c(h(_), [[c(0.70710678,0), c(0.70710678,0)],
                     [c(0.70710678,0), c(-0.70710678,0)]]).
gate_matrix_c(x_gate(_), [[c(0,0), c(1,0)],
                          [c(1,0), c(0,0)]]).
gate_matrix_c(y_gate(_), [[c(0,0), c(0,-1)],
                          [c(0,1), c(0,0)]]).
gate_matrix_c(z_gate(_), [[c(1,0), c(0,0)],
                          [c(0,0), c(-1,0)]]).
gate_matrix_c(t(_), [[c(1,0), c(0,0)],
                     [c(0,0), c(0.70710678,0.70710678)]]).
gate_matrix_c(tdg(_), [[c(1,0), c(0,0)],
                       [c(0,0), c(0.70710678,-0.70710678)]]).
gate_matrix_c(s(_), [[c(1,0), c(0,0)],
                     [c(0,0), c(0,1)]]).
gate_matrix_c(sdg(_), [[c(1,0), c(0,0)],
                       [c(0,0), c(0,-1)]]).
gate_matrix_c(cnot(_,_), [[c(1,0), c(0,0), c(0,0), c(0,0)],
                           [c(0,0), c(1,0), c(0,0), c(0,0)],
                           [c(0,0), c(0,0), c(0,0), c(1,0)],
                           [c(0,0), c(0,0), c(1,0), c(0,0)]]).

% NxN complex identity
identity_c(N, I) :-
    N > 0,
    Dim = N,
    identity_rows(Dim, 0, I).

identity_rows(Dim, Dim, []) :- !.
identity_rows(Dim, Pos, [Row|Rest]) :-
    Pos < Dim,
    identity_row(Dim, Pos, 0, Row),
    Pos1 is Pos + 1,
    identity_rows(Dim, Pos1, Rest).

identity_row(Dim, _, Dim, []) :- !.
identity_row(Dim, Pos, Cur, [C|Rest]) :-
    Cur < Dim,
    (   Cur =:= Pos
    ->  C = c(1,0)
    ;   C = c(0,0)
    ),
    Cur1 is Cur + 1,
    identity_row(Dim, Pos, Cur1, Rest).

% Kronecker product of two matrices
kron(M1, M2, Result) :-
    maplist(kron_rows(M2), M1, Rows),
    append(Rows, Result).

kron_rows(M2, Row1, RowProducts) :-
    maplist(kron_row(M2, Row1), M2, RowProducts).

kron_row(_, Row1, Row2, KronRow) :-
    maplist(kron_elem(Row2), Row1, ElemLists),
    append(ElemLists, KronRow).

kron_elem(Row2, c(R1,I1), RowKron) :-
    maplist(kron_scale(c(R1,I1)), Row2, RowKron).

kron_scale(c(R1,I1), c(R2,I2), c(R3,I3)) :-
    R3 is R1*R2 - I1*I2,
    I3 is R1*I2 + I1*R2.

% Complex matrix multiplication
cmultiply(M1, M2, Result) :-
    M2 = [Row1|_],
    length(Row1, NumCols),
    numlist(0, NumCols, ColIndicesExt),
    append(ColIndices, [_], ColIndicesExt),
    maplist(cmultiply_row(M2, ColIndices), M1, Result).

cmultiply_row(M2, ColIndices, Row, ResultRow) :-
    maplist(cmultiply_cell(M2, Row), ColIndices, ResultRow).

cmultiply_cell(M2, Row, ColIndex, Result) :-
    cget_column(M2, ColIndex, Col),
    cdot_product(Row, Col, Result).

cget_column([], _, []).
cget_column([Row|Matrix], ColIndex, [Elem|ColRest]) :-
    nth0(ColIndex, Row, Elem),
    cget_column(Matrix, ColIndex, ColRest).

cdot_product([], [], c(0,0)).
cdot_product([c(R1,I1)|T1], [c(R2,I2)|T2], Result) :-
    cdot_product(T1, T2, Rest),
    mul_c(c(R1,I1), c(R2,I2), Product),
    add_c(Product, Rest, Result).

% Conjugate transpose
ctranspose(Matrix, CT) :-
    transpose(Matrix, T),
    maplist(conj_row, T, CT).

conj_row(Row, ConjRow) :-
    maplist(conj_c, Row, ConjRow).

% Trace of complex matrix (sum of diagonal M[i][i])
ctrace(M, c(0,0)) :- M == [], !.
ctrace(M, Trace) :-
    length(M, N),
    ctrace_diag(M, 0, N, c(0,0), Trace).

ctrace_diag(_, I, I, Acc, Acc) :- !.
ctrace_diag(M, I, N, Acc, Trace) :-
    nth0(I, M, Row),
    nth0(I, Row, Element),
    add_c(Acc, Element, NewAcc),
    I1 is I + 1,
    ctrace_diag(M, I1, N, NewAcc, Trace).

% Build N-qubit complex unitary from AST
build_synthesis_unitary(AST, N, U) :-
    atom(AST),
    !,
    identity_c(N, U).

build_synthesis_unitary(seq(A, B), N, U) :-
    !,
    build_synthesis_unitary(A, N, UA),
    build_synthesis_unitary(B, N, UB),
    cmultiply(UB, UA, U).

build_synthesis_unitary(AST, N, U) :-
    AST =.. [Op|Args],
    synthesis_gate_matrix(Op, Args, N, U),
    !.

% No fallback — unrecognized ASTs must fail the censor

% Expand a single-gate complex matrix to N-qubit space via Kron
synthesis_gate_matrix(Op, Args, N, FullMatrix) :-
    gate_matrix_c(Term, G),
    Term =.. [Op|ArgTemps],
    length(ArgTemps, ArgLen),
    length(Args, ArgLen),
    % Get qubit positions from args
    find_qubits_for_synthesis(Args, QubitIndices, N),
    % Expand G to N×N via kron
    expand_to_n_qubits(G, QubitIndices, N, FullMatrix).

find_qubits_for_synthesis([], [], _).
find_qubits_for_synthesis([Arg|Args], [Idx|Idxs], N) :-
    atom(Arg),
    atom_concat(q, NumStr, Arg),
    atom_number(NumStr, Num),
    Idx is Num - 1,
    find_qubits_for_synthesis(Args, Idxs, N).

% Expand gate matrix to N-qubit system
% G acts on qubit QIdx (0-indexed) in an N-qubit system
% Result: I_{2^QIdx} ⊗ G ⊗ I_{2^(N-QIdx-1)}
expand_to_n_qubits(G, [QIdx], N, Full) :-
    LeftDim is 1 << QIdx,
    RightDim is 1 << (N - QIdx - 1),
    (   LeftDim =:= 1
    ->  Mid1 = G
    ;   identity_c(LeftDim, LeftI),
        kron(LeftI, G, Mid1)
    ),
    (   RightDim =:= 1
    ->  Full = Mid1
    ;   identity_c(RightDim, RightI),
        kron(Mid1, RightI, Full)
    ).

% Two-qubit gate on adjacent qubits [QIdx1, QIdx1+1]
expand_to_n_qubits(G, [QIdx1, QIdx2], N, Full) :-
    QIdx2 =:= QIdx1 + 1,
    !,
    LeftDim is 1 << QIdx1,
    RightDim is 1 << (N - QIdx2 - 1),
    (   LeftDim =:= 1
    ->  Mid1 = G
    ;   identity_c(LeftDim, LeftI),
        kron(LeftI, G, Mid1)
    ),
    (   RightDim =:= 1
    ->  Full = Mid1
    ;   identity_c(RightDim, RightI),
        kron(Mid1, RightI, Full)
    ).

% Two-qubit gate on non-adjacent qubits
expand_to_n_qubits(G, [Q1, Q2], N, Full) :-
    Q2 > Q1 + 1,
    !,
    Dim is 1 << N,
    LastRow is Dim - 1,
    findall(Row, (
        between(0, LastRow, R),
        findall(Entry, (
            between(0, LastRow, C),
            nonadjacent_entry(G, Q1, Q2, N, R, C, Entry)
        ), Row)
    ), Full).

nonadjacent_entry(G, Q1, Q2, N, R, C, Entry) :-
    nonadjacent_qubits_match(Q1, Q2, N, R, C),
    !,
    BitQ1_R is (R >> (N - Q1 - 1)) /\ 1,
    BitQ2_R is (R >> (N - Q2 - 1)) /\ 1,
    BitQ1_C is (C >> (N - Q1 - 1)) /\ 1,
    BitQ2_C is (C >> (N - Q2 - 1)) /\ 1,
    GateRow is (BitQ1_R << 1) \/ BitQ2_R,
    GateCol is (BitQ1_C << 1) \/ BitQ2_C,
    nth0(GateRow, G, GateRowList),
    nth0(GateCol, GateRowList, Entry).
nonadjacent_entry(_, _, _, _, _, _, c(0,0)).

nonadjacent_qubits_match(Q1, Q2, N, R, C) :-
    nonadjacent_check(0, Q1, Q2, N, R, C).

nonadjacent_check(N, _, _, N, _, _) :- !.
nonadjacent_check(K, Q1, Q2, N, R, C) :-
    (   K =:= Q1
    ;   K =:= Q2
    ),
    !,
    K1 is K + 1,
    nonadjacent_check(K1, Q1, Q2, N, R, C).
nonadjacent_check(K, Q1, Q2, N, R, C) :-
    BitR is (R >> (N - K - 1)) /\ 1,
    BitC is (C >> (N - K - 1)) /\ 1,
    BitR =:= BitC,
    K1 is K + 1,
    nonadjacent_check(K1, Q1, Q2, N, R, C).

% Toffoli (CCNOT) target matrix: 8x8
toffoli_target(Target) :-
    Target = [
        [c(1,0), c(0,0), c(0,0), c(0,0), c(0,0), c(0,0), c(0,0), c(0,0)],
        [c(0,0), c(1,0), c(0,0), c(0,0), c(0,0), c(0,0), c(0,0), c(0,0)],
        [c(0,0), c(0,0), c(1,0), c(0,0), c(0,0), c(0,0), c(0,0), c(0,0)],
        [c(0,0), c(0,0), c(0,0), c(1,0), c(0,0), c(0,0), c(0,0), c(0,0)],
        [c(0,0), c(0,0), c(0,0), c(0,0), c(1,0), c(0,0), c(0,0), c(0,0)],
        [c(0,0), c(0,0), c(0,0), c(0,0), c(0,0), c(1,0), c(0,0), c(0,0)],
        [c(0,0), c(0,0), c(0,0), c(0,0), c(0,0), c(0,0), c(0,0), c(1,0)],
        [c(0,0), c(0,0), c(0,0), c(0,0), c(0,0), c(0,0), c(1,0), c(0,0)]
    ].

% Compute smoothed gate fidelity using component-wise distance
gate_fidelity(UTarget, UCandidate, N, Fidelity) :-
    matrix_distance(UTarget, UCandidate, Dist),
    % Max distance for 8x8 unitary is 16.0 (if completely opposite).
    % We convert distance to a pseudo-fidelity score [0, 1]
    Fidelity is max(0.0, 1.0 - (Dist / 16.0)).

matrix_distance([], [], 0.0).
matrix_distance([R1|M1], [R2|M2], Dist) :-
    matrix_distance(M1, M2, RestDist),
    row_distance(R1, R2, RowDist),
    Dist is RestDist + RowDist.

row_distance([], [], 0.0).
row_distance([c(R1,I1)|T1], [c(R2,I2)|T2], Dist) :-
    row_distance(T1, T2, RestDist),
    DiffR is R1 - R2,
    DiffI is I1 - I2,
    CellDist is sqrt(DiffR*DiffR + DiffI*DiffI),
    Dist is RestDist + CellDist.


% Synthesis censor: evolve toward Toffoli target (3-qubit gate)
synthesis_censor(AST, Penalty) :-
    valid_quantum_structure(AST),
    find_qubits(AST, Qubits),
    (   Qubits == []
    ->  Qs = [q1,q2,q3]
    ;   Qs = Qubits
    ),
    sort_qubits(Qs, SortedQs),
    length(SortedQs, N),
    N =:= 3,
    toffoli_target(TTarget),
    build_synthesis_unitary(AST, N, UCandidate),
    gate_fidelity(TTarget, UCandidate, 8, Fidelity),
    !,
    % Anti-identity penalty via matrix trace:
    % If Tr(U) ≈ 8 (close to identity) but Fidelity < 0.8,
    % the search is stuck in the identity trap — penalize heavily.
    % Using matrix trace avoids the complex-number state-simulator bug.
    ctrace(UCandidate, c(ReTrace, ImTrace)),
    TraceMag is sqrt(ReTrace*ReTrace + ImTrace*ImTrace),
    (   TraceMag > 7.5, Fidelity < 0.8
    ->  IdentityPenalty = 1000.0
    ;   IdentityPenalty = 0.0
    ),
    BasePenalty is 1000.0 * (1.0 - Fidelity) + IdentityPenalty,
    ast_size(AST, Size),
    (   Fidelity > 0.99
    ->  Lambda = 0.005
    ;   lambda_synthesis(Lambda)
    ),
    Penalty is BasePenalty + Size * Lambda.
