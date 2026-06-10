% holography.pl - Constraints for Holographic Principle (AdS/CFT) using clp(r)
:- module(holography, [holography_censor/1, valid_holo_structure/1]).

:- dynamic valid_holo_structure/1.
:- use_module(library(clpr)).

% Surfaces must have strictly positive area
is_area(area).
is_area(X) :-
    number(X),
    { X > 0.0 }.

eval_area(area, 100.0).
eval_area(X, X) :- number(X).

valid_holo(A) :- is_area(A).
valid_holo(entropy(A)) :-
    is_area(A).
valid_holo(info_bound(A)) :-
    is_area(A).

% Bekenstein Bound check: entropy S in area A must satisfy S <= A / 4.0
valid_holo(entropy(S, A)) :-
    is_area(A),
    eval_area(A, AreaVal),
    number(S),
    { S > 0.0, S =< AreaVal / 4.0 }.

holography_censor(AST) :- valid_holo(AST).
holography_censor(AST) :- valid_holo_structure(AST).
