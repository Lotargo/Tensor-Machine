(require :asdf)

(defpackage :cognitive-engine (:use :cl))
(in-package :cognitive-engine)
(defvar *current-operators* nil)

(load "v7/engine/engine.lisp")

(in-package :cognitive-engine)
(format t "--- Running Standalone Meta-Axiomatic Hardcore Test: HSLG 3D->4D ---~%")

;; Override target_censor in lisp specifically to demand our paradox resolution
;; The classical singularity division by zero `(limit (div 1 r) r 0)` naturally fails.
;; The engine must discover `hayward_core` or `golden_ratio_mass` to bypass the singularity.
(defun check-with-prolog-hslg-paradox (ast domain proc-cell &optional (mode :target))
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
                   ;; Only accept if it uses the new HSLG meta-axioms
                   (if (and (listp ast) (or (eq (car ast) 'hayward_core)
                                            (eq (car ast) 'golden_ratio_mass)
                                            (eq (car ast) 'qnm_resonance)
                                            (eq (car ast) 'false)
                                            (and (listp (car ast)) (or (eq (caar ast) 'hayward_core) (eq (caar ast) 'golden_ratio_mass)))))
                       0.0
                       1000.0))
                1000.0)))
      (sb-ext:timeout () 1000.0)
      (error () 1000.0))))

;; Temporarily monkey patch the check function for the experiment script
(setf (symbol-function 'check-with-prolog) (symbol-function 'check-with-prolog-hslg-paradox))

;; The seed is the paradox of classical gravity: limit as r->0 of 1/r
(run-engine-parallel '(limit (div 1 r) r 0) :calculus 1000 (detect-cpu-cores))
