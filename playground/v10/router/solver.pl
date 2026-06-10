:- use_module(library(clpfd)).

% Simple tokenizer
tokenize([], []).
tokenize([C|Cs], [C|Ts]) :- char_type(C, upper), tokenize(Cs, Ts).
tokenize([C|Cs], [N|Ts]) :- char_type(C, digit), get_digits([C|Cs], Digits, Rest), number_chars(N, Digits), tokenize(Rest, Ts).
tokenize([<|Cs], ['<'|Ts]) :- tokenize(Cs, Ts).
tokenize([>|Cs], ['>'|Ts]) :- tokenize(Cs, Ts).
tokenize([!|Cs], ['!'|Ts]) :- tokenize(Cs, Ts).
tokenize([i,n|Cs], [in|Ts]) :- tokenize(Cs, Ts).
tokenize([C|Cs], [C|Ts]) :- member(C, ['+', '-', '*', '=', ',', '[', ']', '.', ':', ';']), tokenize(Cs, Ts).
tokenize([_|Cs], Ts) :- tokenize(Cs, Ts). % ignore spaces

get_digits([C|Cs], [C|Ds], Rest) :- char_type(C, digit), !, get_digits(Cs, Ds, Rest).
get_digits(Rest, [], Rest).

% Parser
equations([E|Es]) --> equation(E), [','], equations(Es).
equations([E]) --> equation(E).

equation(eq(L, R)) --> expr(L), ['='], expr(R).
equation(neq(L, R)) --> expr(L), ['!'], ['='], expr(R).
equation(lt(L, R)) --> expr(L), ['<'], expr(R).
equation(gt(L, R)) --> expr(L), ['>'], expr(R).
equation(in(V, Min, Max)) --> term(var(V)), [in], ['['], term(int(Min)), ['.'], ['.'], term(int(Max)), [']'].

expr(add(A,B)) --> term(A), ['+'], term(B).
expr(sub(A,B)) --> term(A), ['-'], term(B).
expr(mul(A,B)) --> term(A), ['*'], term(B).
expr(T) --> term(T).

term(int(I)) --> [I], { integer(I) }.
term(var(V)) --> [V], { atom(V) }.

eval_ast(int(I), I, _).
eval_ast(var(V), Val, Vars) :- member(V=Val, Vars).
eval_ast(add(A,B), Val, Vars) :- eval_ast(A, VA, Vars), eval_ast(B, VB, Vars), Val #= VA + VB.
eval_ast(sub(A,B), Val, Vars) :- eval_ast(A, VA, Vars), eval_ast(B, VB, Vars), Val #= VA - VB.
eval_ast(mul(A,B), Val, Vars) :- eval_ast(A, VA, Vars), eval_ast(B, VB, Vars), Val #= VA * VB.

apply_eq(eq(L, R), Vars) :-
    eval_ast(L, VL, Vars),
    eval_ast(R, VR, Vars),
    VL #= VR.
apply_eq(neq(L, R), Vars) :-
    eval_ast(L, VL, Vars),
    eval_ast(R, VR, Vars),
    VL #\= VR.
apply_eq(lt(L, R), Vars) :-
    eval_ast(L, VL, Vars),
    eval_ast(R, VR, Vars),
    VL #< VR.
apply_eq(gt(L, R), Vars) :-
    eval_ast(L, VL, Vars),
    eval_ast(R, VR, Vars),
    VL #> VR.
apply_eq(in(V, Min, Max), Vars) :-
    member(V=Val, Vars),
    Val in Min..Max.

apply_eqs([], _).
apply_eqs([E|Es], Vars) :-
    apply_eq(E, Vars),
    apply_eqs(Es, Vars).

vars_in_ast(int(_), []).
vars_in_ast(var(V), [V]).
vars_in_ast(add(A,B), Vs) :- vars_in_ast(A, V1), vars_in_ast(B, V2), append(V1, V2, Vs).
vars_in_ast(sub(A,B), Vs) :- vars_in_ast(A, V1), vars_in_ast(B, V2), append(V1, V2, Vs).
vars_in_ast(mul(A,B), Vs) :- vars_in_ast(A, V1), vars_in_ast(B, V2), append(V1, V2, Vs).

vars_in_eq(eq(L, R), Vs) :- vars_in_ast(L, V1), vars_in_ast(R, V2), append(V1, V2, Vs).
vars_in_eq(neq(L, R), Vs) :- vars_in_ast(L, V1), vars_in_ast(R, V2), append(V1, V2, Vs).
vars_in_eq(lt(L, R), Vs) :- vars_in_ast(L, V1), vars_in_ast(R, V2), append(V1, V2, Vs).
vars_in_eq(gt(L, R), Vs) :- vars_in_ast(L, V1), vars_in_ast(R, V2), append(V1, V2, Vs).
vars_in_eq(in(V, _, _), [V]).
vars_in_eqs([], []).
vars_in_eqs([E|Es], Vs) :- vars_in_eq(E, V1), vars_in_eqs(Es, V2), append(V1, V2, VAll), sort(VAll, Vs).

% Core solver
solve_str_core(Str, Result) :-
    string_chars(Str, Chars),
    tokenize(Chars, Tokens),
    phrase(equations(Eqs), Tokens),
    vars_in_eqs(Eqs, VarNames),
    maplist({}/[Name, Name=_Var]>>true, VarNames, VarPairs),
    apply_eqs(Eqs, VarPairs),
    maplist({}/[(_=Var), Var]>>true, VarPairs, Vars),
    Vars ins -1000..1000,
    label(Vars),
    maplist({}/[(Name=Var), Fmt]>>(format(string(Fmt), "~w=~w", [Name, Var])), VarPairs, FmtList),
    atomic_list_concat(FmtList, ',', Result),
    !.

% Safe wrapper
solve_str(Str, Result) :-
    catch(solve_str_core(Str, Res), _, fail),
    Result = Res.

solve_str(_, 'unsupported_logic').

read_and_solve :-
    read_line_to_string(user_input, Str),
    ( Str == end_of_file -> true
    ; ( solve_str(Str, Result) -> write(Result), nl ; write('unsupported_logic'), nl )
    ).

read_and_solve_loop :-
    repeat,
        read_line_to_string(user_input, Str),
        ( Str == "halt." -> !, write('unsupported_logic'), nl ;
          ( Str == end_of_file -> ! ;
            ( solve_str(Str, Result) -> write(Result), nl ; write('unsupported_logic'), nl ),
            flush_output(user_output),
            fail
          )
        ).
