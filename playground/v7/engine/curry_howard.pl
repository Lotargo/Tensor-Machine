% curry_howard.pl - Curry-Howard Isomorphism Censor
% Checks if the generated dependent types in the proof match the program structure.

% Check if a proof AST is valid and has the correct types.
% Base cases
check_proof(proof(type(int), X)) :- integer(X), !.
check_proof(proof(type(int), X)) :- atom(X), X \= type(_), X \= proof(_, _), X \= add(_, _), X \= sub(_, _), X \= mul(_, _), X \= div(_, _), !.

% Recursive rules for arithmetic operations
% A valid addition of two ints produces an int
check_proof(proof(type(int), add(P1, P2))) :-
    check_proof(P1),
    check_proof(P2),
    P1 = proof(type(int), _),
    P2 = proof(type(int), _).

check_proof(proof(type(int), sub(P1, P2))) :-
    check_proof(P1),
    check_proof(P2),
    P1 = proof(type(int), _),
    P2 = proof(type(int), _).

check_proof(proof(type(int), mul(P1, P2))) :-
    check_proof(P1),
    check_proof(P2),
    P1 = proof(type(int), _),
    P2 = proof(type(int), _).

% For division, it produces an int if inputs are ints (ignoring remainders for this abstract type system)
check_proof(proof(type(int), div(P1, P2))) :-
    check_proof(P1),
    check_proof(P2),
    P1 = proof(type(int), _),
    P2 = proof(type(int), _).

% Censor entry point
curry_censor(AST, Penalty) :-
    (   catch(check_proof(AST), _, fail)
    ->  % The proof holds! Type system validated.
        ast_size(AST, Size),
        Penalty is 0.0 + (Size * 0.1) % Occam's razor
    ;   Penalty = 1000.0 % Invalid type proof
    ).
