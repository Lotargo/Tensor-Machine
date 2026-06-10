;; curry_howard.lisp - Curry-Howard Isomorphism Implementation (Proofs as Programs)
;; Translates base AST into a Dependent Type Proof AST before sending to Prolog.

(in-package :cognitive-engine)

(defun generate-proof (ast)
  "Recursively wraps an AST in a proof(Type, Expr) structure."
  (cond
    ;; Numbers are type int
    ((numberp ast)
     `(proof (type int) ,ast))
    ;; Variables are type int (for this basic implementation)
    ((symbolp ast)
     `(proof (type int) ,ast))
    ;; Operators
    ((listp ast)
     (let ((op (car ast))
           (args (cdr ast)))
       (case op
         ;; Arithmetic operations map two ints to an int
         ((+ - * /)
          (if (= (length args) 2)
              `(proof (type int) (,(case op (+ 'add) (- 'sub) (* 'mul) (/ 'div))
                                  ,(generate-proof (first args))
                                  ,(generate-proof (second args))))
              `(proof (type error) ,ast))) ; Invalid arity
         (t `(proof (type unknown) ,ast)))))
    (t `(proof (type error) ,ast))))
