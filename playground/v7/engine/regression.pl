% regression.pl - Empirical Symbolic Regression using data from CSV
:- use_module(library(csv)).

% Manually evaluate an AST by substituting variables (m1, m2, r)
eval_with_context(m1, M1, _, _, M1) :- !.
eval_with_context(m2, _, M2, _, M2) :- !.
eval_with_context(r, _, _, R_val, R_val) :- !.
eval_with_context(Num, _, _, _, Num) :- number(Num), !.
eval_with_context(Atom, _, _, _, Atom) :- atom(Atom), Atom \= m1, Atom \= m2, Atom \= r, !.

eval_with_context(add(A, B), M1, M2, R_val, R) :-
    eval_with_context(A, M1, M2, R_val, RA),
    eval_with_context(B, M1, M2, R_val, RB),
    number(RA), number(RB),
    R is RA + RB.
eval_with_context(sub(A, B), M1, M2, R_val, R) :-
    eval_with_context(A, M1, M2, R_val, RA),
    eval_with_context(B, M1, M2, R_val, RB),
    number(RA), number(RB),
    R is RA - RB.
eval_with_context(mul(A, B), M1, M2, R_val, R) :-
    eval_with_context(A, M1, M2, R_val, RA),
    eval_with_context(B, M1, M2, R_val, RB),
    number(RA), number(RB),
    R is RA * RB.
eval_with_context(div(A, B), M1, M2, R_val, R) :-
    eval_with_context(A, M1, M2, R_val, RA),
    eval_with_context(B, M1, M2, R_val, RB),
    number(RA), number(RB),
    RB =\= 0.0,
    R is RA / RB.

% Calculate sum of squared errors against CSV rows
calculate_error(_, [], 0.0).
calculate_error(AST, [row(M1, M2, R_val, TargetF)|T], Error) :-
    (   catch(eval_with_context(AST, M1, M2, R_val, PredF), _, fail)
    ->  Diff is PredF - TargetF,
        SqErr is Diff * Diff,
        calculate_error(AST, T, RestError),
        Error is SqErr + RestError
    ;   % Evaluation failed (e.g. division by zero), return high penalty
        Error = 1000.0
    ).

% Entry point for regression domain censor
regression_censor(AST, Penalty) :-
    % Include basic Occam's Razor penalty to penalize bloated trees like m1 * m2 / r / r + 0
    (   catch(csv_read_file('v7/engine/gravity_data.csv', Rows, [functor(row), arity(4)]), _, fail)
    ->  calculate_error(AST, Rows, TotalError),
        (   TotalError > 1000.0
        ->  Penalty = 1000.0
        ;   % Add a small ast_size penalty if error is extremely low (meaning we found it, but want the simplest one)
            ast_size(AST, Size),
            (TotalError < 0.1 -> Penalty is TotalError + (Size * 0.01) ; Penalty = TotalError)
        )
    ;   Penalty = 1000.0 % Fallback if file read fails
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
