% arithmetic.pl - Prolog Arithmetic constraints using clp(r) & Heuristic Penalty Occam's Razor
:- use_module(library(clpr)).

% Evaluate constraints using clp(r) and return Heuristic Penalty
arithmetic_censor(AST, Penalty) :-
    (   (contains_div_zero(AST) ; contains_mul_zero(AST))
    ->  Penalty = 1000.0
    ;   (   \+ valid_arithmetic_structure(AST)
        ->  Penalty = 1000.0
        ;   (   \+ evaluable_in_r(AST)
            ->  Penalty = 950.0
            ;   full_simplify(AST, SimplifiedAST),
                (   AST == SimplifiedAST
                ->  count_redundancies(AST, Count),
                    (   Count > 0
                    ->  Penalty = 1000.0 % Strict identity defense: any redundancy kills the branch
                    ;   Penalty = 0.0  % Perfect AST with no redundancies
                    )
                ;   Penalty = 1000.0 % Strict identity defense: AST could be simplified, kill the branch
                )
            )
        )
    ).

% Full recursive simplify wrapper
full_simplify(Expr, Simplified) :-
    simplify_step(Expr, NextExpr),
    ( Expr == NextExpr -> Simplified = Expr ; full_simplify(NextExpr, Simplified) ).

% Core simplify patterns (Occam's razor logic at algebraic level)
simplify_step(add(X, 0), Res) :- !, simplify_step(X, Res).
simplify_step(add(0, X), Res) :- !, simplify_step(X, Res).
simplify_step(sub(X, 0), Res) :- !, simplify_step(X, Res).
simplify_step(sub(X, X), 0) :- !.
simplify_step(mul(X, 1), Res) :- !, simplify_step(X, Res).
simplify_step(mul(1, X), Res) :- !, simplify_step(X, Res).
simplify_step(mul(_, 0), 0) :- !.
simplify_step(mul(0, _), 0) :- !.
simplify_step(div(X, 1), Res) :- !, simplify_step(X, Res).
simplify_step(div(X, X), 1) :- X \= 0, !.

% Recursive descent for operations
simplify_step(add(X, Y), Res) :-
    simplify_step(X, SX), simplify_step(Y, SY),
    ( number(SX), number(SY) -> Res is SX + SY ; Res = add(SX, SY) ).
simplify_step(sub(X, Y), Res) :-
    simplify_step(X, SX), simplify_step(Y, SY),
    ( number(SX), number(SY) -> Res is SX - SY ; (SX == SY -> Res = 0 ; Res = sub(SX, SY)) ).
simplify_step(mul(X, Y), Res) :-
    simplify_step(X, SX), simplify_step(Y, SY),
    ( number(SX), number(SY) -> Res is SX * SY ;
      (SX == 0 ; SY == 0) -> Res = 0 ;
      (SX == 1 -> Res = SY ; SY == 1 -> Res = SX ; Res = mul(SX, SY)) ).
simplify_step(div(X, Y), Res) :-
    simplify_step(X, SX), simplify_step(Y, SY),
    ( number(SX), number(SY), SY \= 0 -> Res is SX / SY ;
      (SX == SY, SY \= 0) -> Res = 1 ;
      (SY == 1 -> Res = SX ; Res = div(SX, SY)) ).

simplify_step(X, X). % Atom or variable fallback

% Helper to penalize complex trees
ast_depth(Var, 1) :- atom(Var), !.
ast_depth(Num, 1) :- number(Num), !.
ast_depth(add(X, Y), D) :- ast_depth(X, DX), ast_depth(Y, DY), D is max(DX, DY) + 1.
ast_depth(sub(X, Y), D) :- ast_depth(X, DX), ast_depth(Y, DY), D is max(DX, DY) + 1.
ast_depth(mul(X, Y), D) :- ast_depth(X, DX), ast_depth(Y, DY), D is max(DX, DY) + 1.
ast_depth(div(X, Y), D) :- ast_depth(X, DX), ast_depth(Y, DY), D is max(DX, DY) + 1.
ast_depth(_, 10). % fallback

% Invalid structures: Division by zero
contains_div_zero(div(_, 0)).
contains_div_zero(add(X, _)) :- contains_div_zero(X).
contains_div_zero(add(_, Y)) :- contains_div_zero(Y).
contains_div_zero(sub(X, _)) :- contains_div_zero(X).
contains_div_zero(sub(_, Y)) :- contains_div_zero(Y).
contains_div_zero(mul(X, _)) :- contains_div_zero(X).
contains_div_zero(mul(_, Y)) :- contains_div_zero(Y).
contains_div_zero(div(X, _)) :- contains_div_zero(X).
contains_div_zero(div(_, Y)) :- contains_div_zero(Y).

% Invalid structures: Multiplying by zero
contains_mul_zero(mul(0, _)).
contains_mul_zero(mul(_, 0)).
contains_mul_zero(add(X, _)) :- contains_mul_zero(X).
contains_mul_zero(add(_, Y)) :- contains_mul_zero(Y).
contains_mul_zero(sub(X, _)) :- contains_mul_zero(X).
contains_mul_zero(sub(_, Y)) :- contains_mul_zero(Y).
contains_mul_zero(mul(X, _)) :- contains_mul_zero(X).
contains_mul_zero(mul(_, Y)) :- contains_mul_zero(Y).
contains_mul_zero(div(X, _)) :- contains_mul_zero(X).
contains_mul_zero(div(_, Y)) :- contains_mul_zero(Y).

% Recursive Redundancy Counter (Occam's soft constraints)
count_redundancies(add(0, X), N) :- !, count_redundancies(X, N1), N is N1 + 1.
count_redundancies(add(X, 0), N) :- !, count_redundancies(X, N1), N is N1 + 1.
count_redundancies(sub(X, 0), N) :- !, count_redundancies(X, N1), N is N1 + 1.
count_redundancies(sub(X, Y), N) :- X == Y, !, count_redundancies(X, N1), N is N1 + 1.
count_redundancies(mul(1, X), N) :- !, count_redundancies(X, N1), N is N1 + 1.
count_redundancies(mul(X, 1), N) :- !, count_redundancies(X, N1), N is N1 + 1.
count_redundancies(div(X, 1), N) :- !, count_redundancies(X, N1), N is N1 + 1.
count_redundancies(div(X, Y), N) :- X == Y, !, count_redundancies(X, N1), N is N1 + 1.
count_redundancies(Term, N) :-
    compound(Term),
    !,
    Term =.. [_|Args],
    map_redundancies(Args, N).
count_redundancies(_, 0).

map_redundancies([], 0).
map_redundancies([H|T], N) :-
    count_redundancies(H, N1),
    map_redundancies(T, N2),
    N is N1 + N2.

valid_arithmetic_structure(X) :- atom(X).
valid_arithmetic_structure(X) :- integer(X).
valid_arithmetic_structure(add(X, Y)) :- valid_arithmetic_structure(X), valid_arithmetic_structure(Y).
valid_arithmetic_structure(sub(X, Y)) :- valid_arithmetic_structure(X), valid_arithmetic_structure(Y).
valid_arithmetic_structure(mul(X, Y)) :- valid_arithmetic_structure(X), valid_arithmetic_structure(Y).
valid_arithmetic_structure(div(X, Y)) :- valid_arithmetic_structure(X), valid_arithmetic_structure(Y).

% Use clp(r) to ensure evaluating to a real number is physically possible
eval_term(X, X) :- integer(X).
eval_term(X, _) :- atom(X). % Variables evaluate generically
eval_term(add(X, Y), R) :- eval_term(X, RX), eval_term(Y, RY), number(RX), number(RY), {R = RX + RY}.
eval_term(add(X, Y), _) :- \+ (eval_term(X, RX), eval_term(Y, RY), number(RX), number(RY)).
eval_term(sub(X, Y), R) :- eval_term(X, RX), eval_term(Y, RY), number(RX), number(RY), {R = RX - RY}.
eval_term(sub(X, Y), _) :- \+ (eval_term(X, RX), eval_term(Y, RY), number(RX), number(RY)).
eval_term(mul(X, Y), R) :- eval_term(X, RX), eval_term(Y, RY), number(RX), number(RY), {R = RX * RY}.
eval_term(mul(X, Y), _) :- \+ (eval_term(X, RX), eval_term(Y, RY), number(RX), number(RY)).
eval_term(div(X, Y), R) :- eval_term(X, RX), eval_term(Y, RY), number(RX), number(RY), {RY \= 0, R = RX / RY}.
eval_term(div(X, Y), _) :- \+ (eval_term(X, RX), eval_term(Y, RY), number(RX), number(RY)).

evaluable_in_r(AST) :- eval_term(AST, _).
