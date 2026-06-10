% thermo.pl - Constraints for Thermodynamics using clp(r)
:- module(thermo, [thermo_censor/1, valid_thermo_structure/1]).

:- dynamic valid_thermo_structure/1.
:- use_module(library(clpr)).

% Third Law of Thermodynamics: absolute temperatures must be strictly above zero
is_temp(t_hot).
is_temp(t_cold).
is_temp(X) :-
    number(X),
    { X > 0.0 }.

eval_temp(t_hot, 500.0).
eval_temp(t_cold, 300.0).
eval_temp(X, X) :- number(X).

valid_thermo(T) :-
    is_temp(T).

valid_thermo(carnot(Th, Tc)) :-
    is_temp(Th),
    is_temp(Tc),
    eval_temp(Th, H),
    eval_temp(Tc, C),
    % Second Law: Heat must flow from hotter to colder, and both must exceed 0K
    { H > C, H > 0.0, C > 0.0 },
    % Carnot Efficiency must be mathematically valid (0 < E < 1)
    { E = 1.0 - C / H, E > 0.0, E < 1.0 }.

valid_thermo(delta_s(T)) :-
    is_temp(T),
    eval_temp(T, Temp),
    { Temp > 0.0 }.

thermo_censor(AST) :- valid_thermo(AST).
thermo_censor(AST) :- valid_thermo_structure(AST).
