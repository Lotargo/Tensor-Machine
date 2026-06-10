% adversarial.pl - Adversarial Logic (Logic GANs) for robust AST generation

% Attempt evaluation with adversarial test cases
% The Blue team proposes an AST, the Red team (adversary) tries to break it
% by injecting extreme/boundary values into its variables.

% Evaluator helper, similar to regression but doesn't map exactly to target, just checks if it computes cleanly
adversarial_eval(AST, X_val, Result) :-
    % We use the eval_with_context from regression, so we must assume it's loaded
    % or duplicate the logic here. Let's provide a robust local implementation
    % to avoid cross-domain dependency issues if regression is disabled.
    eval_adv(AST, X_val, Result).

eval_adv(x, X_val, X_val) :- !.
eval_adv(Num, _, Num) :- number(Num), !.
eval_adv(Atom, _, Atom) :- atom(Atom), Atom \= x, !.

eval_adv(add(A, B), X, R) :- eval_adv(A, X, RA), eval_adv(B, X, RB), number(RA), number(RB), R is RA + RB.
eval_adv(sub(A, B), X, R) :- eval_adv(A, X, RA), eval_adv(B, X, RB), number(RA), number(RB), R is RA - RB.
eval_adv(mul(A, B), X, R) :- eval_adv(A, X, RA), eval_adv(B, X, RB), number(RA), number(RB), R is RA * RB.
eval_adv(div(A, B), X, R) :- eval_adv(A, X, RA), eval_adv(B, X, RB), number(RA), number(RB), RB =\= 0.0, R is RA / RB.

% The Red team attacks by running the AST on these extreme edge cases
red_team_attacks([0, 1, -1, 100, -100, 9999, -9999]).

run_attacks(_, [], 0.0).
run_attacks(AST, [TestCase | T], Penalty) :-
    (   catch(eval_adv(AST, TestCase, _), _, fail)
    ->  run_attacks(AST, T, Penalty) % Survived this attack
    ;   Penalty = 1000.0 % Attack successful! The AST crashed (e.g. div by zero)
    ).

% Entry point for adversarial censor
adversarial_censor(AST, Penalty) :-
    red_team_attacks(Attacks),
    run_attacks(AST, Attacks, AttackPenalty),
    (   AttackPenalty >= 1000.0
    ->  Penalty = 1000.0
    ;   % If it survived all attacks, give it a baseline penalty based on size
        % (Occam's razor) to prevent infinite bloat.
        ast_size(AST, Size),
        Penalty is Size * 1.0
    ).
