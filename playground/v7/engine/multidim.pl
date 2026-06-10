% multidim.pl - Constraints for Multi-Dimensional Spaces & Tensor Calculus
% Dimensionality type system: 
% - 0 is a scalar
% - vector(N) is an N-dimensional vector
% - matrix(R, C) is a rectangular matrix of size R x C

:- module(multidim, [multidim_censor/1, dim_of/2]).

:- dynamic dim_of/2.

% Ground truth variables and constants
dim_of(s1, 0).
dim_of(s2, 0).
dim_of(0, 0).
dim_of(1, 0).

% Vector representations
dim_of(v2_a, vector(2)).
dim_of(v2_b, vector(2)).
dim_of(v3_a, vector(3)).
dim_of(v3_b, vector(3)).
dim_of(v4_a, vector(4)).

% Matrix representations
dim_of(m2x3, matrix(2, 3)).
dim_of(m3x2, matrix(3, 2)).
dim_of(m3x3, matrix(3, 3)).
dim_of(m4x1, matrix(4, 1)).

% Numbers are scalars
dim_of(X, 0) :- number(X).

% Addition / Subtraction: Both sides must be of the SAME dimension.
dim_of(add(A, B), Dim) :-
    dim_of(A, Dim),
    dim_of(B, Dim).

dim_of(sub(A, B), Dim) :-
    dim_of(A, Dim),
    dim_of(B, Dim).

% Scale (scalar multiplication): A must be a scalar, B can be a vector or matrix.
dim_of(scale(A, B), Dim) :-
    dim_of(A, 0),
    dim_of(B, Dim),
    Dim \= 0.

% Transpose: swaps rows and columns of a matrix
dim_of(transpose(A), matrix(C, R)) :-
    dim_of(A, matrix(R, C)).
dim_of(transpose(A), vector(N)) :-
    dim_of(A, vector(N)).

% Matrix Multiplication (matmul): R x C1 multiplied by C1 x C2 -> R x C2
dim_of(matmul(A, B), matrix(R, C2)) :-
    dim_of(A, matrix(R, C1)),
    dim_of(B, matrix(C1, C2)).

% Dot product: Both must be vectors of the SAME dimension. Result is a scalar.
dim_of(dot(A, B), 0) :-
    dim_of(A, vector(Dim)),
    dim_of(B, vector(Dim)).

% Cross product: defined for 3D space. Returns a 3D vector.
dim_of(cross(A, B), vector(3)) :-
    dim_of(A, vector(3)),
    dim_of(B, vector(3)).

% Norm (length): Argument must be a vector or matrix. Result is a scalar.
dim_of(norm(A), 0) :-
    dim_of(A, Dim),
    (Dim = vector(_) ; Dim = matrix(_, _)).

% The censor accepts the AST if its dimensions can be proven without contradiction
multidim_censor(AST) :-
    dim_of(AST, _).
