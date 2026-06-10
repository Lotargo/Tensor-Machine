% rules.pl - Universal Prolog Mathematical Censor
% Master file routing to specialized modules with Heuristic Penalty outputs

:- consult('arithmetic.pl').
:- consult('calculus.pl').
:- consult('quantum.pl').
:- consult('multidim.pl').
:- consult('relativity.pl').
:- consult('thermo.pl').
:- consult('holography.pl').
:- consult('logic.pl').
:- consult('hardware.pl').
:- consult('regression.pl').
:- consult('adversarial.pl').
:- consult('curry_howard.pl').

% Dispatcher based on Domain returning float Penalty (0.0 is perfect, 1000.0 is worst)

:- dynamic novelty_archive/2.
:- dynamic unified_eval/2.

% Master dispatcher entry point from daemon
censor(Mode, Domain, AST, Penalty) :-
    (   Mode == target
    ->  target_censor(Domain, AST, Penalty)
    ;   novelty_censor(Domain, AST, Penalty)
    ).

% Open-Ended Evolution / Novelty Search Censor
novelty_censor(Domain, AST, Penalty) :-
    % First, verify it's a valid structure using the target_censor (expecting a valid penalty < 1000)
    target_censor(Domain, AST, BasePenalty),
    (   BasePenalty >= 1000.0
    ->  Penalty = 1000.0 % Invalid physics/math gets rejected
    ;   % Valid! Now check if we discovered a novel state (phenotype)
        (   get_state(Domain, AST, State)
        ->  (   novelty_archive(Domain, State)
            ->  % Already discovered. Still valid, but not novel.
                % Calculate AST size to give a slight gradient for golfing known states
                ast_size(AST, Size),
                Penalty is 500.0 + (Size * 2.0)
            ;   % NEW DISCOVERY!
                assertz(novelty_archive(Domain, State)),
                ast_size(AST, Size),
                % Small penalty for size to encourage elegant discoveries
                Penalty is 0.0 + (Size * 0.1)
            )
        ;   % Could not extract a clear state, just use base penalty
            Penalty = BasePenalty
        )
    ).

% Phenotype extraction: Map ASTs to concrete math/physics states
get_state(quantum, AST, State) :-
    find_qubits(AST, RawQubits),
    (   RawQubits = [] -> Qubits = [q1, q2] ; Qubits = RawQubits ),
    sort_qubits(Qubits, SortedQs),
    length(SortedQs, N),
    create_ground_state(N, GroundVec),
    GroundState = [amp(c(1, 0), GroundVec)],
    eval_quantum(AST, SortedQs, GroundState, State).

get_state(arithmetic, AST, State) :-
    eval_term(AST, State).

get_state(logic, AST, State) :-
    % For logic, the state is its truth table (TT, TF, FT, FF)
    logic:eval_bool(AST, true, true, S1),
    logic:eval_bool(AST, true, false, S2),
    logic:eval_bool(AST, false, true, S3),
    logic:eval_bool(AST, false, false, S4),
    State = truth_table(S1, S2, S3, S4).

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

target_censor(arithmetic, AST, Penalty) :-
    !,
    arithmetic_censor(AST, Penalty).

target_censor(quantum, AST, Penalty) :-
    !,
    quantum_censor(AST, Penalty).

target_censor(calculus, AST, Penalty) :-
    !,
    (   calculus_censor(AST)
    ->  Penalty = 0.0
    ;   Penalty = 1000.0
    ).

target_censor(regression, AST, Penalty) :-
    !,
    regression_censor(AST, Penalty).

target_censor(adversarial, AST, Penalty) :-
    !,
    adversarial_censor(AST, Penalty).

target_censor(curry_howard, AST, Penalty) :-
    !,
    curry_censor(AST, Penalty).

target_censor(multidim, AST, Penalty) :-
    !,
    (   multidim:multidim_censor(AST)
    ->  Penalty = 0.0
    ;   Penalty = 1000.0
    ).

target_censor(relativity, AST, Penalty) :-
    !,
    (   relativity:relativity_censor(AST)
    ->  Penalty = 0.0
    ;   Penalty = 1000.0
    ).

target_censor(thermo, AST, Penalty) :-
    !,
    (   thermo:thermo_censor(AST)
    ->  Penalty = 0.0
    ;   Penalty = 1000.0
    ).

target_censor(holography, AST, Penalty) :-
    !,
    (   holography:holography_censor(AST)
    ->  Penalty = 0.0
    ;   Penalty = 1000.0
    ).

target_censor(logic, AST, Penalty) :-
    !,
    logic:logic_censor(AST, Penalty).

target_censor(hardware, AST, Penalty) :-
    !,
    hardware:hardware_censor(AST, Penalty).

target_censor(synthesis, AST, Penalty) :-
    !,
    synthesis_censor(AST, Penalty).

target_censor(unified, AST, Penalty) :-
    !,
    unified_censor(AST, Penalty).

unified_censor(AST, Penalty) :-
    unified_eval(AST, Penalty).

% Base cases
unified_eval(X, 0.0) :- atom(X), !.
unified_eval(X, 0.0) :- number(X), !.

% Quantum sub-expressions (evaluate as a whole quantum block)
unified_eval(AST, Penalty) :-
    is_quantum_expr(AST),
    !,
    quantum_censor(AST, Penalty).

% Thermodynamics sub-expressions
unified_eval(AST, Penalty) :-
    is_thermo_expr(AST),
    !,
    (   thermo:thermo_censor(AST)
    ->  Penalty = 0.0
    ;   Penalty = 1000.0
    ).

% Relativity sub-expressions
unified_eval(AST, Penalty) :-
    is_rel_expr(AST),
    !,
    (   relativity:relativity_censor(AST)
    ->  Penalty = 0.0
    ;   Penalty = 1000.0
    ).

% Holography sub-expressions
unified_eval(AST, Penalty) :-
    is_holo_expr(AST),
    !,
    (   holography:holography_censor(AST)
    ->  Penalty = 0.0
    ;   Penalty = 1000.0
    ).

% Multidimensional sub-expressions
unified_eval(AST, Penalty) :-
    is_multidim_expr(AST),
    !,
    (   multidim:multidim_censor(AST)
    ->  Penalty = 0.0
    ;   Penalty = 1000.0
    ).

% Calculus sub-expressions
unified_eval(AST, Penalty) :-
    is_calc_expr(AST),
    !,
    (   calculus_censor(AST)
    ->  Penalty = 0.0
    ;   Penalty = 1000.0
    ).

% Logic sub-expressions
unified_eval(AST, Penalty) :-
    is_logic_expr(AST),
    !,
    logic:logic_censor(AST, Penalty).

% Arithmetic operators (recursive traversal + soft Occam's Razor)
unified_eval(add(A, B), Penalty) :-
    !,
    unified_eval(A, PA),
    unified_eval(B, PB),
    (   (A == 0 ; B == 0)
    ->  PR = 20.0
    ;   PR = 0.0
    ),
    Penalty is PA + PB + PR.

unified_eval(sub(A, B), Penalty) :-
    !,
    unified_eval(A, PA),
    unified_eval(B, PB),
    (   (B == 0 ; A == B)
    ->  PR = 20.0
    ;   PR = 0.0
    ),
    Penalty is PA + PB + PR.

unified_eval(mul(A, B), Penalty) :-
    !,
    (   (A == 0 ; B == 0)
    ->  Penalty = 1000.0
    ;   unified_eval(A, PA),
        unified_eval(B, PB),
        (   (A == 1 ; B == 1)
        ->  PR = 20.0
        ;   PR = 0.0
        ),
        Penalty is PA + PB + PR
    ).

unified_eval(div(A, B), Penalty) :-
    !,
    (   B == 0
    ->  Penalty = 1000.0
    ;   unified_eval(A, PA),
        unified_eval(B, PB),
        (   (B == 1 ; A == B)
        ->  PR = 20.0
        ;   PR = 0.0
        ),
        Penalty is PA + PB + PR
    ).

% Catch-all fallback for other compound terms
unified_eval(AST, Penalty) :-
    compound(AST),
    !,
    AST =.. [_|Args],
    map_unified_eval(Args, Penalty).

map_unified_eval([], 0.0).
map_unified_eval([H|T], Penalty) :-
    unified_eval(H, P1),
    map_unified_eval(T, P2),
    Penalty is P1 + P2.

% Helper Domain Classifiers
is_quantum_expr(seq(_, _)).
is_quantum_expr(tensor(_, _)).
is_quantum_expr(h(_)).
is_quantum_expr(x_gate(_)).
is_quantum_expr(y_gate(_)).
is_quantum_expr(z_gate(_)).
is_quantum_expr(cnot(_, _)).
is_quantum_expr(oracle(_, _)).

is_thermo_expr(carnot(_, _)).
is_thermo_expr(delta_s(_)).

is_rel_expr(gamma(_)).
is_rel_expr(add_vel(_, _)).
is_rel_expr(warp_drive(_, _)).
is_rel_expr(tachyon(_)).

is_holo_expr(entropy(_)).
is_holo_expr(info_bound(_)).

is_multidim_expr(matmul(_, _)).
is_multidim_expr(transpose(_)).
is_multidim_expr(cross(_, _)).
is_multidim_expr(dot(_, _)).
is_multidim_expr(scale(_, _)).
is_multidim_expr(norm(_)).
is_multidim_expr(wormhole(_, _)).
is_multidim_expr(teleport(_, _)).
is_multidim_expr(fold(_, _)).

is_calc_expr(deriv(_, _)).
is_calc_expr(int(_, _)).
is_calc_expr(sin(_)).
is_calc_expr(cos(_)).
is_calc_expr(limit(_, _, _)).
is_calc_expr(div(_, _)).

is_logic_expr(and(_, _)).
is_logic_expr(or(_, _)).
is_logic_expr(xor(_, _)).
is_logic_expr(implies(_, _)).
is_logic_expr(not(_)).

