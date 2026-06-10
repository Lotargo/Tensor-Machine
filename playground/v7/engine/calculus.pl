% calculus.pl - Prolog Calculus invariants (Derivatives and Integrals)
:- dynamic valid_calculus_structure/1.

calculus_censor(AST) :-
    valid_calculus_structure(AST).

% Define valid calculus structural bounds and semantic correctness
valid_calculus_structure(X) :- atom(X).
valid_calculus_structure(X) :- integer(X).
valid_calculus_structure(neg(X)) :- valid_calculus_structure(X).
valid_calculus_structure(add(X, Y)) :- valid_calculus_structure(X), valid_calculus_structure(Y).
valid_calculus_structure(sub(X, Y)) :- valid_calculus_structure(X), valid_calculus_structure(Y).
valid_calculus_structure(mul(X, Y)) :- valid_calculus_structure(X), valid_calculus_structure(Y).
valid_calculus_structure(div(X, Y)) :- valid_calculus_structure(X), valid_calculus_structure(Y).
valid_calculus_structure(sin(X)) :- valid_calculus_structure(X).
valid_calculus_structure(cos(X)) :- valid_calculus_structure(X).

% Deriv: deriv(Expression, Variable)
valid_calculus_structure(deriv(Expr, Var)) :-
    atom(Var), % Deriving with respect to a single variable
    valid_calculus_structure(Expr),
    % Formally prove that we can compute a valid symbolic derivative
    diff(Expr, Var, D),
    valid_calculus_structure(D).

% Int: int(Expression, Variable)
valid_calculus_structure(int(Expr, Var)) :-
    atom(Var), % Integrating with respect to a single variable
    valid_calculus_structure(Expr).

% Symbolic Differentiation Rules
diff(X, X, 1) :- !.
diff(C, X, 0) :-
    atomic(C),
    C \= X,
    !.
diff(neg(U), X, neg(DU)) :-
    !,
    diff(U, X, DU).
diff(add(U, V), X, add(DU, DV)) :-
    !,
    diff(U, X, DU),
    diff(V, X, DV).
diff(sub(U, V), X, sub(DU, DV)) :-
    !,
    diff(U, X, DU),
    diff(V, X, DV).
diff(mul(U, V), X, add(mul(U, DV), mul(V, DU))) :-
    !,
    diff(U, X, DU),
    diff(V, X, DV).
diff(div(U, V), X, div(sub(mul(V, DU), mul(U, DV)), mul(V, V))) :-
    !,
    diff(U, X, DU),
    diff(V, X, DV).
diff(sin(U), X, mul(cos(U), DU)) :-
    !,
    diff(U, X, DU).
diff(cos(U), X, mul(neg(sin(U)), DU)) :-
    !,
    diff(U, X, DU).
% Graceful fallback: if it contains an unresolved sub-derivative, propagate it
diff(deriv(Expr, V), X, deriv(D, V)) :-
    !,
    diff(Expr, X, D).
