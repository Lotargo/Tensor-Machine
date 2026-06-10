(require :asdf)
(make-package :cognitive-engine-noauto)
(load "v7/engine/engine.lisp")
(in-package :cognitive-engine)

(defun run-toffoli-experiment (&key (iterations 50000) (threads (detect-cpu-cores)))
  (let ((log-path "v7/experiments/toffoli_results.log"))
    (with-open-file (*standard-output* log-path
                     :direction :output
                     :if-exists :supersede
                     :if-does-not-exist :create)
      (format t "========================================================~%")
      (format t "  QUANTUM SYNTHESIS: TOFFOLI GATE DECOMPOSITION~%")
      (format t "  Target: CCNOT (3-qubit Controlled-Controlled-NOT)~%")
      (format t "  Gate set: {H, T, T*, S, S*, X, CNOT}~%")
      (format t "========================================================~%")
      (format t "  Fitness = { 1000*(1-Fidelity) + lambda*Size }~%")
      (format t "========================================================~%~%")
      (format t "Configuration:~%")
      (format t "  Starting AST : (seq (h q1) (seq (h q2) (h q3)))~%")
      (format t "  Domain       : synthesis~%")
      (format t "  Iterations   : ~D~%" iterations)
      (format t "  Threads      : ~D~%" threads)
      (format t "  Target       : Toffoli (CCNOT)~%~%")
      (force-output *standard-output*)

      (run-engine-parallel '(seq (h q1) (seq (h q2) (h q3))) :synthesis iterations threads)

      (format t "~%========================================================~%")
      (format t "  EXPERIMENT COMPLETE~%")
      (format t "========================================================~%"))
    (format t "Results saved to ~A~%" log-path)))

(run-toffoli-experiment :iterations 50000)
