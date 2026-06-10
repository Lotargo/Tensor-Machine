% relativity.pl - Constraints for Special Relativity using clp(r)
:- module(relativity, [relativity_censor/1, valid_rel/1, valid_speed/1, eval_vel/2]).

:- dynamic valid_rel/1.
:- dynamic valid_speed/1.
:- dynamic eval_vel/2.
:- use_module(library(clpr)).

% Speeds strictly less than speed of light c = 1.0
valid_speed(X) :-
    number(X),
    { X > -1.0, X < 1.0 }.
valid_speed(v1).
valid_speed(v2).
valid_speed(add_vel(A, B)) :-
    valid_speed(A),
    valid_speed(B),
    eval_vel(add_vel(A, B), W),
    { W > -1.0, W < 1.0 }.

% Evaluate velocities
eval_vel(X, X) :- number(X).
eval_vel(v1, 0.5).
eval_vel(v2, 0.8).
eval_vel(add_vel(A, B), W) :-
    eval_vel(A, U),
    eval_vel(B, V),
    { W = (U + V) / (1.0 + U * V) }.

% Relativity expressions
valid_rel(gamma(V)) :-
    valid_speed(V),
    eval_vel(V, S),
    % Ensure Lorentz factor is physically real (speed is sub-light)
    { S > -1.0, S < 1.0 }.
valid_rel(V) :-
    valid_speed(V).

relativity_censor(AST) :-
    valid_rel(AST).
