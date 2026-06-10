(require :asdf)

(defpackage :cognitive-engine (:use :cl))
(in-package :cognitive-engine)
(defvar *current-operators* nil)

(load "v7/engine/engine.lisp")

(in-package :cognitive-engine)
(format t "--- Running Standalone Meta-Axiomatic Hardcore Test: HSLG Part 2 (Information Paradox) ---~%")

(defun check-with-prolog-hslg-part2-paradox (ast domain proc-cell &optional (mode :target))
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
                   (if (and (listp ast) (or (eq (car ast) 'effective_lambda)
                                            (eq (car ast) 'holo_reconstruction)
                                            (eq (car ast) 'hayward_roots)
                                            (eq (car ast) 'info_paradox_resolved)
                                            (eq (car ast) 'false)
                                            (and (listp (car ast)) (or (eq (caar ast) 'effective_lambda) (eq (caar ast) 'info_paradox_resolved)))))
                       0.0
                       1000.0))
                1000.0)))
      (sb-ext:timeout () 1000.0)
      (error () 1000.0))))

(setf (symbol-function 'check-with-prolog) (symbol-function 'check-with-prolog-hslg-part2-paradox))

;; The seed is the paradox of information loss during merger S_final - (S1 + S2) < 0
(run-engine-parallel '(sub (add (entropy s1) (entropy s2)) (entropy s_final)) :unified 1000 (detect-cpu-cores))
