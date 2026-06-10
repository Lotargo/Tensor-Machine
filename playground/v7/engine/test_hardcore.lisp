(require :asdf)

(defpackage :cognitive-engine (:use :cl))
(in-package :cognitive-engine)
(defvar *current-operators* nil)

(load "v7/engine/engine.lisp")

(in-package :cognitive-engine)
(format t "--- Running Standalone Meta-Axiomatic Hardcore Test (Paradox) ---~%")
;; We will test the specific hardcore AST.
;; Since the domain rules and check-with-prolog handle the penalty,
;; if it successfully discovers `wormhole` or `teleport`, it will get a 0.0 penalty.
(run-engine-parallel '(add v3_a m3x3) :multidim 50 (detect-cpu-cores))
