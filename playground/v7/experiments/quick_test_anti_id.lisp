(require :asdf)
(make-package :cognitive-engine-noauto)
(load "v7/engine/engine.lisp")
(in-package :cognitive-engine)

(defun quick-anti-identity-test ()
  (let ((log-path "v7/experiments/toffoli_quick.log"))
    (with-open-file (*standard-output* log-path
                     :direction :output
                     :if-exists :supersede
                     :if-does-not-exist :create)
      (format t "=== Quick Anti-Identity Test ===~%")
      (format t "Domain: synthesis, Target: Toffoli (CCNOT)~%")
      (format t "Iterations: 1000, Threads: 4~%~%")
      (force-output *standard-output*)
      (run-engine-parallel '(seq (h q1) (seq (h q2) (h q3))) :synthesis 1000 4)
      (format t "~%=== Done ===~%"))
    (format t "Results saved to ~A~%" log-path)))

(quick-anti-identity-test)
(sb-ext:quit)
