(defun run-prolog-test (expr expected)
  (let* ((process (sb-ext:run-program "swipl"
                                      (list "-q" "-s" "router/solver.pl"
                                            "-g" "read_and_solve, halt.")
                                      :input :stream
                                      :output :stream
                                      :wait nil
                                      :search t))
         (input (sb-ext:process-input process))
         (output (sb-ext:process-output process)))
    (write-line expr input)
    (force-output input)
    (close input)
    (let ((result (read-line output nil "")))
      (sb-ext:process-close process)
      (assert (string= result expected)))))

(format t "Testing Prolog Logic Solver Math Correctness...~%")
(run-prolog-test "X+Y=10,X-Y=2" "X=6,Y=4")
(run-prolog-test "2+2=X" "X=4")
(run-prolog-test "X*2=10" "X=5")

(format t "Testing Prolog Logic Solver CSP Extensions...~%")
(run-prolog-test "A<10,A>8" "A=9")
(run-prolog-test "A!=10,A in [9..11],A!=11" "A=9")

(format t "Prolog Tests Passed.~%")
