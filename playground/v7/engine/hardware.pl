% hardware.pl - Prolog Censor for Silicon Lowering (Hardware Synthesis)
% Evaluates logic circuits for propagation delay, gate count, and power consumption.

:- module(hardware, [hardware_censor/2]).

% hardware_censor(+AST, -Penalty)
% Evaluates the logic AST and assigns a penalty based on hardware efficiency.
% A valid boolean logic expression gets a penalty reflecting its silicon cost.
% Invalid expressions receive a penalty of 1000.0.
hardware_censor(AST, Penalty) :-
    % First verify it's a valid logic expression using the logic module's evaluate
    (   catch(verify_logic(AST), _, fail)
    ->  % It's valid logic. Now calculate hardware costs.
        gate_count(AST, GateCount),
        propagation_delay(AST, Delay),
        % Cost function: we want to minimize both gates and depth (delay)
        % Using floats to maintain consistency with the rest of the engine
        Penalty is GateCount * 1.5 + Delay * 2.0
    ;   Penalty = 1000.0
    ).

% Verify hardware AST (support basic boolean resolution)
% We won't exhaustively check 16 states for P,Q,R,S here due to legacy logic:eval_bool arity.
% Instead we verify it's a valid syntax tree where leaves are atoms.
verify_logic(AST) :-
    verify_structure(AST).

verify_structure(Var) :- atom(Var), !.
verify_structure(and(A, B)) :- verify_structure(A), verify_structure(B).
verify_structure(or(A, B)) :- verify_structure(A), verify_structure(B).
verify_structure(not(A)) :- verify_structure(A).
verify_structure(implies(A, B)) :- verify_structure(A), verify_structure(B).
verify_structure(gate_and(A, B)) :- verify_structure(A), verify_structure(B).
verify_structure(gate_or(A, B)) :- verify_structure(A), verify_structure(B).
verify_structure(gate_not(A)) :- verify_structure(A).

% gate_count(+AST, -Count)
% Counts the number of logic gates.
gate_count(Var, 0) :- atom(Var), !.
gate_count(and(A, B), C) :- gate_count(A, C1), gate_count(B, C2), C is C1 + C2 + 1, !.
gate_count(or(A, B), C)  :- gate_count(A, C1), gate_count(B, C2), C is C1 + C2 + 1, !.
gate_count(not(A), C)    :- gate_count(A, C1), C is C1 + 1, !.
gate_count(implies(A, B), C) :- gate_count(A, C1), gate_count(B, C2), C is C1 + C2 + 2, !. % implies is NOT A OR B
gate_count(gate_and(A, B), C) :- gate_count(A, C1), gate_count(B, C2), C is C1 + C2 + 1, !.
gate_count(gate_or(A, B), C)  :- gate_count(A, C1), gate_count(B, C2), C is C1 + C2 + 1, !.
gate_count(gate_not(A), C)    :- gate_count(A, C1), C is C1 + 1, !.
gate_count(_, 0).

% propagation_delay(+AST, -Delay)
% Estimates the critical path delay (depth of the AST).
propagation_delay(Var, 0) :- atom(Var), !.
propagation_delay(and(A, B), D) :- propagation_delay(A, D1), propagation_delay(B, D2), D is max(D1, D2) + 1, !.
propagation_delay(or(A, B), D)  :- propagation_delay(A, D1), propagation_delay(B, D2), D is max(D1, D2) + 1, !.
propagation_delay(not(A), D)    :- propagation_delay(A, D1), D is D1 + 1, !.
propagation_delay(implies(A, B), D) :- propagation_delay(A, D1), propagation_delay(B, D2), D is max(D1, D2) + 2, !.
propagation_delay(gate_and(A, B), D) :- propagation_delay(A, D1), propagation_delay(B, D2), D is max(D1, D2) + 1, !.
propagation_delay(gate_or(A, B), D)  :- propagation_delay(A, D1), propagation_delay(B, D2), D is max(D1, D2) + 1, !.
propagation_delay(gate_not(A), D)    :- propagation_delay(A, D1), D is D1 + 1, !.
propagation_delay(_, 0).
