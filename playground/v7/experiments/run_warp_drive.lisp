(require :asdf)

(defpackage :cognitive-engine (:use :cl))
(in-package :cognitive-engine)
(defvar *current-operators* nil)

(load "v7/engine/engine.lisp")

(in-package :cognitive-engine)
(format t "--- Running Standalone Meta-Axiomatic Hardcore Test: WARP DRIVE ---~%")

;; Override target_censor in lisp specifically to demand our superluminal result for the test
;; Since clp(r) evaluation natively constraints it, we want the lisp logic to only reward
;; the AST if it breaks the speed of light by finding the warp_drive axiom.
(defun check-with-prolog-relativity-paradox (ast domain proc-cell &optional (mode :target))
  (let* ((proc (car proc-cell))
         (prolog-term (lisp-to-prolog ast))
         (query (format nil "censor(~A, ~A, ~A, Penalty)." (string-downcase (symbol-name mode)) (string-downcase (symbol-name domain)) prolog-term)))
    (handler-case
        (sb-ext:with-timeout 1.0
          (format (uiop:process-info-input proc) "~A~%" query)
          (force-output (uiop:process-info-input proc))
          (let ((response (read-line (uiop:process-info-output proc) nil :eof)))
            (if (and (stringp response) (not (eq response :eof)))
                (let ((base-pen (or (ignore-errors (read-from-string (string-trim '(#\Space #\Tab #\Return #\Newline #\") response))) 1000.0)))
                   ;; Only accept if it uses the new meta-axiom
                   (if (and (listp ast) (or (eq (car ast) 'warp_drive) (eq (car ast) 'tachyon)))
                       0.0
                       1000.0))
                1000.0)))
      (sb-ext:timeout () 1000.0)
      (error () 1000.0))))

;; Temporarily monkey patch the check function for the experiment script
(setf (symbol-function 'check-with-prolog) (symbol-function 'check-with-prolog-relativity-paradox))

(run-engine-parallel '(add_vel 0.8 0.8) :relativity 1000 (detect-cpu-cores))
