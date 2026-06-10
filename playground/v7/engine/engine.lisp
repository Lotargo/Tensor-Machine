(require :asdf)

(defpackage :cognitive-engine
  (:use :cl))
(in-package :cognitive-engine)

;; Load Meta-Evolution extensions
(load "v7/engine/meta_evolve.lisp")
(load "v7/engine/ontology_generator.lisp")
(load "v7/engine/verilog_emitter.lisp")
(load "v7/engine/curry_howard.lisp")

;; =======================================================================
;; 0b. Hardware-Accelerated Regression (CPU SIMD / GPU CUDA)
;; =======================================================================

;; Ensure sonata-engine package exists at read time.  Try loading the real
;; wrapper; on failure create a minimal stub.
(eval-when (:compile-toplevel :load-toplevel :execute)
  (unless (ignore-errors
            (progn (ql:quickload :cffi)
                   (load "cuda/cffi.lisp")
                   t))
    (unless (find-package :sonata-engine)
      (defpackage :sonata-engine
        (:use)
        (:export :flatten-ast :eval-ast-sse :load-engine :cpu-simd-level)))
    ;; Define stub functions using INTERN to avoid sonata-engine: at read-time.
    (flet ((def-stub (name fn)
             (let ((sym (intern (string name) :sonata-engine)))
               (fmakunbound sym)
               (setf (symbol-function sym) fn))))
      (def-stub "flatten-ast"    (lambda (expr) (declare (ignore expr)) (values #() #())))
      (def-stub "eval-ast-sse"   (lambda (&rest args) (declare (ignore args)) 0.0))
      (def-stub "load-engine"    (lambda (&key backend) (declare (ignore backend)) nil))
      (def-stub "cpu-simd-level" (lambda () -1)))
    (warn "[SONATA] Wrapper not loaded; stub package created.")))

(defvar *sonata-engine-available* nil
  "T when the sonata_cuda DLL loaded successfully.")
(defvar *gravity-data* nil
  "Float vector of gravity data, row-major: [M1, M2, r, F] per row.")
(defvar *gravity-n-rows* 20)
(defvar *gravity-n-vars* 4)

(defun %load-sonata-engine ()
  "Load sonata_cuda.dll. Non-fatal if unavailable."
  (handler-case
      (progn
        (sonata-engine:load-engine :backend :cpu)
        (format t "[SONATA] CPU SIMD backend loaded (level ~D)~%"
                (sonata-engine:cpu-simd-level))
        (setf *sonata-engine-available* t))
    (error (e)
      (format t "[SONATA] DLL not available: ~A — falling back to Prolog~%" e)
      (setf *sonata-engine-available* nil))))

(defun %load-gravity-data ()
  "Read gravity_data.csv into *gravity-data* float vector."
  (setf *gravity-data*
        (with-open-file (s "v7/engine/gravity_data.csv")
          (let* ((n-rows *gravity-n-rows*)
                 (n-vars *gravity-n-vars*)
                 (vec (make-array (* n-rows n-vars) :element-type 'single-float))
                 (idx 0))
            (loop for line = (read-line s nil nil) while line
                  do (let ((line-lisp (substitute #\Space #\, line)))
                       (loop for val in (read-from-string (concatenate 'string "(" line-lisp ")"))
                             do (setf (aref vec idx) (coerce val 'single-float))
                                (incf idx))))
            vec))))

(defun %translate-regression-ast (ast)
  "Map regression AST operators (add/sub/mul/div) and vars (m1/m2/r) to
flatten-ast format (+/-/*// and v0/v1/v2)."
  (flet ((map-var (sym)
           (case sym
             (m1 'v0) (m2 'v1) (r 'v2)
             (t sym))))
    (cond
      ((numberp ast) ast)
      ((symbolp ast) (map-var ast))
      ((listp ast)
       (let ((op (car ast))
             (args (cdr ast)))
         (cons (case op
                 (add '+)
                 (sub '-)
                 (mul '*)
                 (div '/)
                 (t op))
               (mapcar #'%translate-regression-ast args)))))))

;; =======================================================================
;; 1. Domains Configuration
;; =======================================================================

(defvar *current-operators* '(+ - * /))
(defvar *current-variables* '(x y z))
(defvar *current-constants* '(0 1 2 5 10))

(defun set-domain (domain-name)
  (case domain-name
    (:arithmetic
     (setf *current-operators* '(+ - * /))
     (setf *current-variables* '(x y z))
     (setf *current-constants* '(0 1 2 5 10)))
    (:calculus
     (setf *current-operators* '(+ - * / deriv int sin cos))
     (setf *current-variables* '(x t))
     (setf *current-constants* '(0 1 -1)))
    (:quantum
     (setf *current-operators* '(tensor seq h x_gate y_gate z_gate cnot oracle))
     (setf *current-variables* '(q1 q2 q3))
     (setf *current-constants* '(1 0)))
    (:regression
     (setf *current-operators* '(add sub mul div))
     (setf *current-variables* '(m1 m2 r))
     (setf *current-constants* '(1)))
    (:adversarial
     (setf *current-operators* '(add sub mul div))
     (setf *current-variables* '(x))
     (setf *current-constants* '(0 1 2 5 10)))
    (:curry_howard
     (setf *current-operators* '(+ - * /))
     (setf *current-variables* '(x y))
     (setf *current-constants* '(0 1 2)))
     (:synthesis
      (setf *current-operators* '(seq h t tdg s sdg x_gate y_gate z_gate cnot))
      (setf *current-variables* '(q1 q2 q3))
      (setf *current-constants* nil))
    (t (error "Unknown domain"))))

;; =======================================================================
;; 2. Lisp: Generator and Mutator of DNA
;; =======================================================================

(defun random-element (list)
  (nth (random (length list)) list))

(defun single-arg-operator-p (op)
  (member op '(h t tdg s sdg x_gate y_gate z_gate transpose norm gamma delta_s oracle)))

(defun compatible-arity-ops (ops current-arity)
  "Filter operators to those matching the given arity."
  (remove-if-not
    (lambda (op)
      (= (if (single-arg-operator-p op) 1 2) current-arity))
    ops))

(defun mutate (ast)
  "Mutates the AST by randomly changing operators, variables, or numbers, and occasionally growing."
  (if (atom ast)
      (if (< (random 100) 15) ; 15% chance to grow an atom into a subtree
          (let ((op (and *current-operators* (random-element *current-operators*))))
            (if op
                (let ((arity (if (single-arg-operator-p op) 1 2)))
                  (cons op (loop repeat arity
                                 collect (if (< (random 10) 5)
                                             (if *current-variables* (random-element *current-variables*) 0)
                                             (if *current-constants* (random-element *current-constants*) 0)))))
                ast))
          (if (numberp ast)
              (if *current-constants* (random-element *current-constants*) ast)
              (if *current-variables* (random-element *current-variables*) ast)))
      (let ((op (car ast))
            (args (cdr ast)))
        (if (< (random 10) 4) ; 40% chance to mutate the operator
            (if *current-operators*
                (let* ((current-arity (length args))
                       (compatible-ops (compatible-arity-ops *current-operators* current-arity))
                       (new-op (and compatible-ops (random-element compatible-ops))))
                  (if new-op
                      (cons new-op args)
                      ast))
                ast)
            ;; Otherwise, mutate a random child node
            (if args
                (let ((mutated-args (copy-list args))
                      (idx (random (length args))))
                  ;; For thermodynamics `0` is technically a number, so make sure
                  ;; random mutations don't get stuck if we just randomly pick a leaf
                  (if (and (atom (nth idx mutated-args)) (< (random 10) 5))
                      (setf (nth idx mutated-args) (if *current-variables* (random-element *current-variables*) (nth idx mutated-args)))
                      (setf (nth idx mutated-args) (mutate (nth idx mutated-args))))
                  (cons op mutated-args))
                ast)))))

(defun crossover (ast1 ast2)
  "Simple crossover: replace a random subtree in ast1 with ast2."
  (if (atom ast1)
      ast2
      (if (< (random 10) 3)
          ast2
          (let ((new-args (copy-list (cdr ast1)))
                (idx (random (length (cdr ast1)))))
            (setf (nth idx new-args) (crossover (nth idx new-args) ast2))
            (cons (car ast1) new-args)))))

;; =======================================================================
;; 2b. Synthesis-Specific Mutation Operators

(defparameter *structural-templates* (list
  '(seq (h q3) (seq (cnot q2 q3) (h q3)))
  '(seq (t q1) (seq (t q2) (t q3)))
  '(seq (cnot q1 q2) (seq (cnot q2 q3) (cnot q1 q2)))

  '(seq (h q3)
     (seq (cnot q2 q3)
       (seq (tdg q3)
         (seq (cnot q1 q3)
           (seq (t q3)
             (seq (cnot q2 q3)
               (seq (tdg q3)
                 (seq (cnot q1 q3)
                   (seq (t q2)
                     (seq (t q3)
                       (seq (cnot q1 q2)
                         (seq (h q3)
                           (seq (t q1)
                             (seq (tdg q2)
                               (cnot q1 q2)))))))))))))))


  '(seq (h q3)
     (seq (cnot q2 q3)
       (seq (tdg q3)
         (seq (cnot q1 q3)
           (seq (t q3)
             (seq (cnot q2 q3)
               (seq (tdg q3)
                 (seq (cnot q1 q3)
                   (seq (t q2)
                     (seq (t q3)
                       (seq (cnot q1 q2)
                         (seq (h q3)
                           (seq (t q1)
                             (seq (tdg q2)
                               (cnot q1 q2)))))))))))))))

))

;; =======================================================================


(defun instantiate-template (template)
  "Instantiate a structural template by replacing generic qubits with random current ones."
  (cond
    ((null template) nil)
    ((atom template)
     (if (and (symbolp template) (member template '(q1 q2 q3)))
         (if *current-variables* (random-element *current-variables*) template)
         template))
    (t (cons (instantiate-template (car template))
             (instantiate-template (cdr template))))))

(defun random-gate ()
  "Generate a random quantum gate (never seq)."
  (let* ((ops (remove 'seq *current-operators*))
         (op (random-element ops)))
    (if (single-arg-operator-p op)
        (list op (random-element *current-variables*))
        (list op (random-element *current-variables*) (random-element *current-variables*)))))

(defun gate-insertion-mutation (ast)
  "Insert a random gate at a random leaf in a binary-nested seq circuit."
  (if (and (listp ast) (eq (car ast) 'seq))
      (let ((left (cadr ast))
            (right (caddr ast)))
        (if (< (random 2) 1)
            (list 'seq (gate-insertion-mutation left) right)
            (list 'seq left (gate-insertion-mutation right))))
      (let ((new-gate (random-gate)))
        (if (< (random 2) 1)
            (list 'seq new-gate ast)
            (list 'seq ast new-gate)))))

(defun gate-deletion-mutation (ast)
  "Remove a random leaf gate from a binary-nested seq circuit."
  (if (and (listp ast) (eq (car ast) 'seq))
      (let ((left (cadr ast))
            (right (caddr ast)))
        (let ((r (random 100)))
          (cond
            ((< r 15) left)
            ((< r 30) right)
            ((< r 65) (list 'seq (gate-deletion-mutation left) right))
            (t (list 'seq left (gate-deletion-mutation right))))))
      ast))

(defun random-subtree (ast)
  "Extract a random subtree from a binary-nested circuit."
  (if (and (listp ast) (eq (car ast) 'seq) (< (random 10) 6))
      (random-subtree (random-element (cdr ast)))
      ast))

(defun circuit-crossover (ast1 ast2)
  "Crossover: replace a random subtree of ast1 with a random subtree of ast2."
  (let ((sub2 (random-subtree ast2)))
    (crossover ast1 sub2)))

(defun bulk-mutate (ast)
  "Apply 3-5 random insertions/deletions for larger search jumps."
  (let ((n (+ 3 (random 3)))
        (current ast))
    (loop repeat n do
      (setf current
            (let ((r (random 100)))
              (cond
                ((< r 40) (gate-insertion-mutation current))
                ((< r 65) (gate-deletion-mutation current))
                (t (mutate current))))))
    current))

(defun random-gate-sequence (length)
  "Generate a random contiguous sequence of gates of the given length."
  (if (<= length 1)
      (random-gate)
      (list 'seq (random-gate) (random-gate-sequence (1- length)))))

(defun bulk-swap-mutation (ast)
  "Replace a random subtree with a new random contiguous sequence of gates (length 2 to 5) or a template."
  (let* ((use-template (and *structural-templates* (< (random 100) 25)))
         (new-seq (if use-template
                      (instantiate-template (random-element *structural-templates*))
                      (random-gate-sequence (+ 2 (random 4))))))
    (if (or (atom ast) (= (length (cdr ast)) 0))
        new-seq
        (if (< (random 10) 3) ; 30% chance to replace this node
            new-seq
            (let ((new-args (copy-list (cdr ast)))
                  (idx (random (length (cdr ast)))))
              (setf (nth idx new-args) (bulk-swap-mutation (nth idx new-args)))
              (cons (car ast) new-args))))))

(defun synthesis-mutate (ast &key bulk)
  "Synthesis-specific mutation dispatcher with insertion/deletion/bulk-swap/standard.
   When :bulk is true, applies 3-5 mutations at once for escaping local minima."
  (if bulk
      (bulk-mutate ast)
      (let ((r (random 100)))
        (cond
          ((< r 25) (gate-insertion-mutation ast))
          ((< r 45) (gate-deletion-mutation ast))
          ((< r 55) (bulk-swap-mutation ast))
          (t (mutate ast))))))

(defun send-lambda (proc-cell val)
  "Update the lambda coefficient in the worker's Prolog daemon for λ-annealing."
  (let ((proc (car proc-cell)))
    (handler-case
        (sb-ext:with-timeout 1.0
          (format (uiop:process-info-input proc) "set_lambda(~F).~%" val)
          (force-output (uiop:process-info-input proc))
          (read-line (uiop:process-info-output proc) nil :eof))
      (sb-ext:timeout ()
        (format t "[TIMEOUT] Prolog daemon hung on set_lambda. Restarting...~%")
        (ignore-errors (uiop:terminate-process proc :urgent t))
        (let ((new-proc (uiop:launch-program (list "swipl" "-q" "-s" "v7/engine/daemon.pl")
                                             :input :stream
                                             :output :stream)))
          (setf (car proc-cell) new-proc)))
      (error (e)
        (format t "[ERROR] set_lambda pipe error: ~A~%" e)))))

;; =======================================================================
;; 3. Prolog Interface: Mathematical Censor
;; =======================================================================

(defvar *adf-library* nil)
(defparameter *novelty-search-mode* :auto)


(defun expand-adfs (ast)
  "Recursively expands any ADF symbols in the AST into their actual bodies."
  (cond
    ((null ast) nil)
    ((atom ast)
     (let ((body (assoc ast *adf-library*)))
       (if body
           (expand-adfs (cdr body))
           ast)))
    ((listp ast)
     (let ((body (assoc (car ast) *adf-library*)))
       (if body
           (expand-adfs (cdr body))
           (cons (car ast) (mapcar #'expand-adfs (cdr ast))))))))

(defun lisp-to-prolog (ast)
  "Translates Lisp AST into Prolog term syntax after expanding any ADFs."
  (let ((expanded (expand-adfs ast)))
    (cond
      ((numberp expanded) (format nil "~A" expanded))
      ((symbolp expanded) (string-downcase (symbol-name expanded)))
      ((listp expanded)
       (let ((op (car expanded))
             (args (cdr expanded)))
         (let ((prolog-op (case op
                            (+ "add")
                            (- "sub")
                            (* "mul")
                            (/ "div")
                            (t (string-downcase (symbol-name op))))))
           (format nil "~A(~{~A~^, ~})" prolog-op (mapcar #'lisp-to-prolog args))))))))

(defun ast-size (ast)
  "Count AST nodes (matching Prolog's ast_size/2)."
  (cond
    ((atom ast) 1)
    (t (1+ (reduce #'+ (cdr ast) :key #'ast-size)))))

(defun check-with-prolog (ast domain proc-cell &optional (mode :target))
  "Sends the AST to the running SWI-Prolog daemon with a 1.0s timeout.
   Returns the penalty score (a float between 0.0 and 1000.0) if successful, or 1000.0 if failed.
   For :regression domain with :target mode, uses hardware-accelerated eval if available."
  ;; Hardware-accelerated regression eval
  (when (and *sonata-engine-available*
             (eq domain :regression)
             (eq mode :target))
    (return-from check-with-prolog
      (handler-case
          (let* ((v-ast (%translate-regression-ast ast))
                 (data (or *gravity-data* (%load-gravity-data)))
                 (n-rows *gravity-n-rows*)
                 (n-vars *gravity-n-vars*))
            (multiple-value-bind (opcodes args)
                (sonata-engine:flatten-ast v-ast)
              (let* ((sse (sb-int:with-float-traps-masked (:invalid :divide-by-zero :overflow)
                            (sonata-engine:eval-ast-sse
                             opcodes args data n-rows n-vars)))
                     (clamped (min sse 1000.0)))
                (if (< clamped 0.1)
                    (+ clamped (* (ast-size ast) 0.01))
                    clamped))))
        (error (e)
          (format t "[SONATA] eval error: ~A — returning 1000.0~%" e)
          1000.0))))
  ;; Standard Prolog path for all other domains/modes
  (let* ((proc (car proc-cell))
         (prepared-ast (if (eq domain :curry_howard) (generate-proof ast) ast))
         (prolog-term (lisp-to-prolog prepared-ast))
         (query (format nil "censor(~A, ~A, ~A, Penalty)." (string-downcase (symbol-name mode)) (string-downcase (symbol-name domain)) prolog-term)))
    (handler-case
        (sb-ext:with-timeout 1.0 ; 1.0 second (1000 milliseconds)
          (format (uiop:process-info-input proc) "~A~%" query)
          (force-output (uiop:process-info-input proc))
          (let ((response (read-line (uiop:process-info-output proc) nil :eof)))
            (if (and (stringp response) (not (eq response :eof)))
                (or (ignore-errors (read-from-string (string-trim '(#\Space #\Tab #\Return #\Newline #\") response)))
                    1000.0)
                1000.0)))
      (sb-ext:timeout ()
        (format t "[TIMEOUT] Prolog daemon hung on query: ~A. Forcefully restarting...~%" query)
        (ignore-errors (uiop:terminate-process proc :urgent t))
        (let ((new-proc (uiop:launch-program (list "swipl" "-q" "-s" "v7/engine/daemon.pl")
                                             :input :stream
                                             :output :stream)))
          (setf (car proc-cell) new-proc))
        1000.0)
      (error (e)
        (format t "[ERROR] Prolog daemon pipe error: ~A. Restarting daemon...~%" e)
        (ignore-errors (uiop:terminate-process proc :urgent t))
        (let ((new-proc (uiop:launch-program (list "swipl" "-q" "-s" "v7/engine/daemon.pl")
                                             :input :stream
                                             :output :stream)))
          (setf (car proc-cell) new-proc))
        1000.0))))

;; =======================================================================
;; 4. Main Universal Cognitive Engine Loop (Multi-Threaded)
;; =======================================================================

(defun get-domain-config (domain-name)
  (case domain-name
    (:arithmetic
     (values '(+ - * /) '(x y z) '(0 1 2 5 10)))
    (:calculus
     (values '(+ - * / deriv int sin cos) '(x t) '(0 1 -1)))
    (:quantum
     (values '(seq h x_gate y_gate z_gate cnot oracle) '(q1 q2) nil))
    (:multidim
     (values '(add sub dot cross scale norm matmul transpose) '(m2x3 m3x2 m3x3 m4x1 s1 s2) '(0 1)))
    (:relativity
     (values '(gamma add_vel) '(v1 v2) '(0 0.1 0.5 0.8 0.9 1.2)))
    (:thermo
     (values '(carnot delta_s) '(t_hot t_cold) '(0 100 300 500)))
    (:holography
     (values '(entropy info_bound) '(area volume) '(0 1 2 5 10)))
    (:logic
     (values '(and or xor implies not) '(p q) '(true false)))
    (:hardware
     (values '(and or implies not gate_and gate_or gate_not) '(p q r s) '(true false)))
    (:regression
     (values '(add sub mul div) '(m1 m2 r) '(1)))
    (:adversarial
     (values '(add sub mul div) '(x) '(0 1 2 5 10)))
    (:curry_howard
     (values '(+ - * /) '(x y) '(0 1 2)))
    (:synthesis
     (values '(seq h t tdg s sdg x_gate y_gate z_gate cnot) '(q1 q2 q3) nil))
    (:unified
     (values '(+ - * / deriv int sin cos seq h cnot x_gate y_gate z_gate matmul transpose cross dot scale norm gamma add_vel carnot delta_s entropy info_bound and or implies not)
             '(x y z t q1 q2 q3 m2x3 m3x2 m3x3 m4x1 s1 s2 v1 v2 t_hot t_cold area volume p q)
             '(0 1 2 5 10 -1 0.1 0.5 0.8 0.9 1.2 100 300 500 true false)))
    (t (error (format nil "Unknown domain: ~A" domain-name)))))

(defun worker-loop (base-ast domain iterations thread-id migration-pools migration-mutexes)
  (multiple-value-bind (ops vars consts) (get-domain-config domain)
    ;; Dynamic bindings of special variables are thread-local in SBCL
    (let* ((*current-operators* ops)
           ;; Synthesis domain: gate args must be atom qubits, not ADF subexpressions
           (*current-variables* (if (eq domain :synthesis)
                                    vars
                                    (append vars (mapcar #'car *adf-library*))))
           (*current-constants* consts)
           (proc (uiop:launch-program (list "swipl" "-q" "-s" "v7/engine/daemon.pl")
                                      :input :stream
                                      :output :stream))
           (proc-cell (list proc))
            (best-ast base-ast)
            ;; Saved target best — protected from novelty mode overwrites
            (saved-best-ast base-ast)
            (current-mode (if (eq *novelty-search-mode* t) :novelty :target))
            (best-penalty (check-with-prolog base-ast domain proc-cell current-mode))
            (saved-best-penalty best-penalty)
            (actual-iters 0)
            (stagnation-counter 0)
            (novelty-discoveries 0)
            (current-temp 250.0)
            (recent-accepts 0)
            (recent-moves 0))
       (let ((current-ast base-ast))
         (loop for i from 1 to iterations
               while (or (> best-penalty 0.0) (eq current-mode :novelty))
               do (incf actual-iters)
                  (incf stagnation-counter)

                  ;; Auto Mode Switching Logic
                  (when (eq *novelty-search-mode* :auto)
                      (cond
                       ((and (eq current-mode :target) (> stagnation-counter 50))
                       (format t "[Thread ~D] Stagnation detected. Switching to EXPLORATION (Novelty) mode.~%" thread-id)
                       ;; Save target best before switching
                       (setf saved-best-ast best-ast)
                       (setf saved-best-penalty best-penalty)
                       (setf current-mode :novelty)
                       (setf stagnation-counter 0)
                       (setf novelty-discoveries 0)
                       (setf best-penalty 1000.0))
                      ((and (eq current-mode :novelty) (>= novelty-discoveries 5))
                       (format t "[Thread ~D] Sufficient exploration done. Switching back to TARGET mode.~%" thread-id)
                       (setf current-mode :target)
                       (setf stagnation-counter 0)
                       ;; Restore best from target mode (not novelty discoveries)
                       (setf best-ast saved-best-ast)
                       (setf best-penalty saved-best-penalty)
                       (setf current-ast saved-best-ast))))

                   ;; λ-annealing: keep constant 0.01 for 80% (exploration), then anneal 0.01 → 0.50
                   (when (and (eq domain :synthesis) (eq current-mode :target)
                              (= (mod i (max 1 (floor iterations 50))) 0))
                     (let* ((progress (/ i iterations))
                            (lambda-val (if (< progress 0.8)
                                            0.0
                                            (* 0.50 (/ (- progress 0.8) 0.2)))))
                       (send-lambda proc-cell (min lambda-val 0.50))))

                   (let* ((use-bulk (and (eq domain :synthesis)
                                        (eq current-mode :target)
                                        (> stagnation-counter 15)))
                          (hypothesis (if (eq domain :synthesis)
                                         (synthesis-mutate current-ast :bulk use-bulk)
                                         (mutate current-ast)))
                         (penalty (check-with-prolog hypothesis domain proc-cell current-mode)))

                   ;; Attempt Meta-Evolution mutation
                   (when (and (> penalty 0.0) (< (random 100) 50))
                     (cond
                       ((eq domain :multidim)
                        (multiple-value-bind (meta-ast meta-penalty meta-axiom)
                            (meta-mutate-axiom proc-cell domain current-ast best-penalty #'check-with-prolog)
                          (when meta-ast
                            (if (and (listp meta-ast)
                                     (or (eq (car meta-ast) 'wormhole)
                                         (eq (car meta-ast) 'teleport)
                                         (eq (car meta-ast) 'fold)))
                                (setf meta-penalty 0.0))
                            (setf hypothesis meta-ast)
                            (setf penalty meta-penalty))))
                       ((eq domain :relativity)
                        (multiple-value-bind (meta-ast meta-penalty meta-axiom)
                            (meta-mutate-axiom-relativity proc-cell domain current-ast best-penalty #'check-with-prolog)
                          (when meta-ast
                            (if (and (listp meta-ast)
                                     (or (eq (car meta-ast) 'warp_drive)
                                         (eq (car meta-ast) 'tachyon)))
                                (setf meta-penalty 0.0))
                            (setf hypothesis meta-ast)
                            (setf penalty meta-penalty))))
                       ((eq domain :calculus)
                        (multiple-value-bind (meta-ast meta-penalty meta-axiom)
                            (meta-mutate-axiom-hslg proc-cell domain current-ast best-penalty #'check-with-prolog)
                          (when meta-ast
                            (if (and (listp meta-ast)
                                     (or (eq (car meta-ast) 'hayward_core)
                                         (eq (car meta-ast) 'golden_ratio_mass)
                                         (eq (car meta-ast) 'qnm_resonance)))
                                (setf meta-penalty 0.0))
                            (setf hypothesis meta-ast)
                            (setf penalty meta-penalty))))
                       ((eq domain :unified)
                        (multiple-value-bind (meta-ast meta-penalty meta-axiom)
                            (meta-mutate-axiom-hslg-part2 proc-cell domain current-ast best-penalty #'check-with-prolog)
                          (when meta-ast
                            (if (and (listp meta-ast)
                                     (or (eq (car meta-ast) 'effective_lambda)
                                         (eq (car meta-ast) 'holo_reconstruction)
                                         (eq (car meta-ast) 'hayward_roots)
                                         (eq (car meta-ast) 'info_paradox_resolved)
                                         (and (listp (car meta-ast)) (eq (caar meta-ast) 'effective_lambda))))
                                (setf meta-penalty 0.0))
                            (setf hypothesis meta-ast)
                            (setf penalty meta-penalty))))))

                   ;; Log Discoveries in Novelty Mode
                   (when (and (eq current-mode :novelty) (< penalty 100.0) (< penalty best-penalty))
                      (format t "[NOVELTY DISCOVERY] Thread ~D found novel state in ~A! AST: ~A (Penalty: ~,2F)~%" thread-id domain hypothesis penalty)
                      (incf novelty-discoveries)
                      (with-open-file (stream "discoveries.log" :direction :output :if-exists :append :if-does-not-exist :create)
                        (format stream "[NOVELTY] Thread ~D | Domain: ~A | Penalty: ~,2F | AST: ~A~%" thread-id domain penalty hypothesis)))

                   (let* ((is-better (or (< penalty best-penalty)
                                         (and (= penalty best-penalty) (< (length (format nil "~A" hypothesis)) (length (format nil "~A" best-ast))))))
                          (accepted-uphill nil)
                          (delta (- penalty best-penalty)))
                     (when (and (not is-better) (eq domain :synthesis))
                       ;; Adaptive Simulated Annealing for synthesis domain
                       (incf recent-moves)
                       (let ((prob (exp (- (/ delta current-temp)))))
                         (when (< (random 1.0) prob)
                           (setf accepted-uphill t)
                           (incf recent-accepts)))
                       (when (>= recent-moves 100)
                         (let ((ratio (/ recent-accepts (float recent-moves))))
                           (if (< ratio 0.2)
                               (setf current-temp (* current-temp 1.1))
                               (setf current-temp (* current-temp 0.9))))
                         (setf recent-moves 0)
                         (setf recent-accepts 0)))

                     (if is-better
                         (progn
                           (setf stagnation-counter 0)
                           (setf best-ast hypothesis)
                           (setf best-penalty penalty)
                           (setf current-ast hypothesis)
                           (when (and (eq current-mode :target) (= best-penalty 0.0))
                             (format t "[Thread ~D] [CONVERGED] Iteration ~D: ~A (Penalty: 0.0)~%" thread-id i best-ast)
                             (return)))
                         (progn
                           (if accepted-uphill
                               (setf current-ast hypothesis)
                               (when (and (not (eq domain :synthesis)) (< (random 100) 15))
                                 (setf current-ast hypothesis)))))))
                 
                 ;; Periodic Island Migration (Ring Topology)
                 (when (and migration-pools migration-mutexes (> actual-iters 0) (= 0 (mod actual-iters 50)))
                   (let* ((num-pools (length migration-pools))
                          (target-idx (mod thread-id num-pools))
                          (mutex (aref migration-mutexes target-idx))
                          (pool (aref migration-pools target-idx)))
                     (sb-thread:with-mutex (mutex)
                       (unless (member best-ast pool :test #'equal)
                         (when (< (length pool) 5)
                           (setf (aref migration-pools target-idx) (cons best-ast pool))
                           (format t "[Thread ~D] Migrated best AST to Thread ~D: ~A~%" thread-id (1+ target-idx) best-ast)))))
                   
                   (let* ((our-idx (1- thread-id))
                          (mutex (aref migration-mutexes our-idx))
                          (immigrants nil))
                     (sb-thread:with-mutex (mutex)
                       (setf immigrants (aref migration-pools our-idx))
                       (setf (aref migration-pools our-idx) nil))
                      (dolist (immigrant immigrants)
                        (let ((imm-penalty (check-with-prolog immigrant domain proc-cell current-mode)))
                          (if (< imm-penalty best-penalty)
                              (progn
                                (setf best-ast immigrant)
                                (setf best-penalty imm-penalty)
                                (setf current-ast immigrant)
                                (setf stagnation-counter 0)
                                (format t "[Thread ~D] Accepted immigrant! New best: ~A (Penalty: ~,2F)~%" thread-id best-ast best-penalty))
                              (when (< (random 100) 50)
                                (let* ((child (if (eq domain :synthesis)
                                                 (circuit-crossover current-ast immigrant)
                                                 (crossover current-ast immigrant)))
                                       (child-penalty (check-with-prolog child domain proc-cell current-mode)))
                                  (when (< child-penalty best-penalty)
                                    (setf best-ast child)
                                    (setf best-penalty child-penalty)
                                    (setf current-ast child)
                                    (setf stagnation-counter 0)
                                    (format t "[Thread ~D] Crossover with immigrant! New best: ~A (Penalty: ~,2F)~%" thread-id child best-penalty)))))))))))
      ;; Clean up the Prolog daemon
      (let ((active-proc (car proc-cell)))
        (format (uiop:process-info-input active-proc) "quit~%")
        (ignore-errors (force-output (uiop:process-info-input active-proc)))
        (uiop:wait-process active-proc))
      (cons best-ast actual-iters))))

(defun synthesize-hybrids (winners domain desired-hybrids max-iterations)
  (when (< (length winners) 2)
    (return-from synthesize-hybrids winners))
  (format t "~%--- Final Synthesis Phase (Island Crossover) ---~%")
  (multiple-value-bind (ops vars consts) (get-domain-config domain)
    (let* ((*current-operators* ops)
           (*current-variables* (append vars (mapcar #'car *adf-library*)))
           (*current-constants* consts)
           (proc (uiop:launch-program (list "swipl" "-q" "-s" "v7/engine/daemon.pl")
                                      :input :stream
                                      :output :stream))
           (proc-cell (list proc))
           (hybrids nil))
      (loop for i from 1 to max-iterations
            while (< (length hybrids) desired-hybrids)
            do (let* ((parent1 (random-element winners))
                      (parent2 (random-element winners))
                                             (child (if (eq domain :synthesis)
                                 (circuit-crossover parent1 parent2)
                                 (crossover parent1 parent2))))
                 ;; Only accept the child if it's actually different from parents and has 0.0 penalty
                 (when (and (not (equal child parent1))
                            (not (equal child parent2))
                            (= (check-with-prolog child domain proc-cell) 0.0))
                   (format t "[SYNTHESIS] Found valid hybrid at iteration ~D: ~A~%" i child)
                   (push child hybrids))))
      (let ((active-proc (car proc-cell)))
        (format (uiop:process-info-input active-proc) "quit~%")
        (ignore-errors (force-output (uiop:process-info-input active-proc)))
        (uiop:wait-process active-proc))
      hybrids)))

(defun run-engine-parallel (base-ast domain iterations num-threads)
  (setf *discovered-axioms* nil)
  (format t "=== Starting Parallel Cognitive Engine v7 ===~%")
  (format t "Domain: ~A~%" domain)
  (format t "Base AST: ~A~%" base-ast)
  (format t "Threads: ~D~%" num-threads)

  (let* ((start-time (get-internal-real-time))
         (migration-pools (make-array num-threads :initial-element nil))
         (migration-mutexes (map 'vector (lambda (x) (declare (ignore x)) (sb-thread:make-mutex)) (make-list num-threads)))
         (threads (loop for i from 1 to num-threads
                        collect (let ((thread-id i))
                                  (sb-thread:make-thread
                                   (lambda ()
                                     (worker-loop base-ast domain iterations thread-id migration-pools migration-mutexes))))))
         (thread-results (mapcar #'sb-thread:join-thread threads))
         (end-time (get-internal-real-time))
         (elapsed-seconds (/ (- end-time start-time) (coerce internal-time-units-per-second 'double-float)))
         (total-iterations (loop for res in thread-results sum (cdr res)))
         (winners (remove-duplicates (remove nil (mapcar #'car thread-results)) :test #'equal)))

    ;; Dynamic Subroutine Extraction: Register successful island winners as new ADF symbols
    ;; Skip for synthesis — gate args must be atom qubits, not subexpressions
    (unless (eq domain :synthesis)
      (dolist (w winners)
        (let ((adf-sym (intern (format nil "ADF-~D" (1+ (length *adf-library*))))))
          (push (cons adf-sym w) *adf-library*)
          (format t "[ADF MANAGER] Extracted and registered dynamic subroutine: ~A -> ~A~%" adf-sym w))))
    
    (format t "~%==================================================~%")
    (format t "      v7 ENGINE PERFORMANCE DASHBOARD~%")
    (format t "==================================================~%")
    (format t "Total Elapsed Time    : ~,4F seconds~%" elapsed-seconds)
    (format t "Logical CPU Cores     : ~D~%" num-threads)
    (format t "Total ASTs Evaluated  : ~D~%" total-iterations)
    (format t "Throughput (AST/sec)  : ~,2F~%" (if (> elapsed-seconds 0) (/ total-iterations elapsed-seconds) 0))
    (format t "Avg Latency per AST   : ~,4F ms~%" (if (> total-iterations 0) (/ (* elapsed-seconds 1000) total-iterations) 0))
    (format t "==================================================~%~%")

    (if winners
        (progn
          (format t "Found valid hypotheses from Islands:~%")
          (loop for res in winners do (format t "  ~A~%" res))
          (let ((hybrids (synthesize-hybrids winners domain 3 20000)))
            (when hybrids
              (format t "Ultimate Hybrids:~%")
              (loop for h in hybrids do (format t "  ~A~%" h))
              ;; Generate LaTeX Ontology for the best discovery
              (generate-latex-ontology domain base-ast *discovered-axioms* (car hybrids) total-iterations elapsed-seconds))))
        (format t "No valid hypotheses found.~%"))

    ;; Silicon Lowering
    (when (and winners (member domain '(:hardware :logic)))
      (let* ((best (car winners))
             (mod-name (format nil "synth_~A" (string-downcase (symbol-name domain))))
             (path (format nil "v7/hardware/~A.v" mod-name)))
        (uiop:run-program "mkdir -p v7/hardware" :ignore-error-status t)
        (emit-verilog best mod-name path)))
    ))

(defun detect-cpu-cores ()
  "Attempts to detect the number of logical CPU cores. Fallbacks to 4 if detection fails."
  (let ((env-cores (uiop:getenv "NUMBER_OF_PROCESSORS")))
    (if env-cores
        (or (ignore-errors (parse-integer env-cores)) 4)
        (let ((proc-cores (ignore-errors
                            (with-open-file (s "/proc/cpuinfo" :if-does-not-exist nil)
                              (if s
                                  (loop for line = (read-line s nil nil)
                                        while line
                                        count (search "processor" line))
                                  nil)))))
          (or proc-cores 4)))))

(defun contains-operator-p (operators ast)
  "Recursively checks if the AST contains any of the given operators/symbols."
  (cond
    ((null ast) nil)
    ((atom ast)
     (and (or (symbolp ast) (stringp ast) (characterp ast))
          (member ast operators :test #'string-equal)))
    (t (or (contains-operator-p operators (car ast))
           (contains-operator-p operators (cdr ast))))))

(defun detect-domain-by-ast (ast)
  "Auto-detects the scientific domain based on operators and variables present in the AST."
  (let ((domains nil))
    (when (contains-operator-p '(gamma add_vel v1 v2) ast) (push :relativity domains))
    (when (contains-operator-p '(carnot delta_s t_hot t_cold) ast) (push :thermo domains))
    (when (contains-operator-p '(entropy info_bound area volume) ast) (push :holography domains))
    (when (contains-operator-p '(tensor seq h x_gate y_gate z_gate cnot oracle q1 q2 q3) ast) (push :quantum domains))
    (when (contains-operator-p '(matmul transpose cross dot scale norm m2x3 m3x2 m3x3 m4x1 v2_a v2_b v3_a v3_b v4_a) ast) (push :multidim domains))
    (when (contains-operator-p '(deriv int sin cos t) ast) (push :calculus domains))
    (when (contains-operator-p '(and or xor implies not p q true false) ast) (push :logic domains))
    (when (contains-operator-p '(gate_and gate_or gate_not r s) ast) (push :hardware domains))
    (when (contains-operator-p '(proof type int) ast) (push :curry_howard domains))
    (let ((unique-domains (remove-duplicates domains)))
      (cond
        ((> (length unique-domains) 1) :unified)
        ((= (length unique-domains) 1) (car unique-domains))
        (t :arithmetic)))))

(defun parse-cli-args (args)
  "Parses command line arguments into variables."
  (let ((domain nil)
        (base-ast nil)
        (iterations 20000)
        (threads nil))
    (loop for arg-list = args then (cdr arg-list)
          while arg-list
          do (let ((arg (car arg-list))
                   (next-arg (cadr arg-list)))
               (cond
                 ((or (string= arg "--domain") (string= arg "-d"))
                  (setf domain (intern (string-upcase next-arg) :keyword))
                  (setf arg-list (cdr arg-list)))
                 ((or (string= arg "--base") (string= arg "-b"))
                  (setf base-ast (read-from-string next-arg))
                  (setf arg-list (cdr arg-list)))
                 ((or (string= arg "--iterations") (string= arg "-i"))
                  (setf iterations (parse-integer next-arg))
                  (setf arg-list (cdr arg-list)))
                 ((or (string= arg "--threads") (string= arg "-t"))
                  (setf threads (parse-integer next-arg))
                  (setf arg-list (cdr arg-list))))))
    (values domain base-ast iterations threads)))

(defun main ()
  ;; Initialize hardware-accelerated regression eval
  (%load-sonata-engine)
  (let ((args (uiop:command-line-arguments)))
    (if args
        (multiple-value-bind (domain base-ast iterations threads) (parse-cli-args args)
          (unless base-ast
            (error "Missing required argument: --base <lisp-expression> (e.g. -b \"(h (h q1))\")"))
          (let* ((auto-domain (or domain (detect-domain-by-ast base-ast)))
                 (num-threads (or threads (detect-cpu-cores))))
            (format t "~%=== Running Custom CLI Task ===~%")
            (if domain
                (format t "Domain: ~A~%" domain)
                (format t "Domain: ~A (Auto-detected)~%" auto-domain))
            (format t "Base AST: ~A~%" base-ast)
            (format t "Iterations: ~D~%" iterations)
            (format t "Threads: ~D~%" num-threads)
            (run-engine-parallel base-ast auto-domain iterations num-threads)))
        ;; If no CLI arguments, run the default full multi-domain demo suite!
        (let ((arith-base '(+ x 0))
              (calc-base '(deriv (sin x t) x))
              (quant-base '(seq (h q2) (cnot q2 q1)))
              (num-threads (detect-cpu-cores)))
          (format t "~%[SYSTEM] Detected logical CPU cores: ~D~%" num-threads)

          (format t "~%--- Running Arithmetic Domain ---~%")
          (run-engine-parallel arith-base :arithmetic 200 num-threads)

          (format t "~%--- Running Calculus Domain ---~%")
          (run-engine-parallel calc-base :calculus 200 num-threads)

          (format t "~%--- Running Quantum Domain ---~%")
          (run-engine-parallel quant-base :quantum 200 num-threads)

          (format t "~%--- Running Multi-Dimensional Tensor Domain ---~%")
          (run-engine-parallel '(add v3_a v3_b) :multidim 2000 num-threads)
          (format t "~%--- Running Meta-Axiomatic Hardcore Test (Paradox) ---~%")
          (run-engine-parallel '(cross v3_a v3_b) :multidim 50 num-threads)

          (format t "~%--- Running Relativity Domain ---~%")
          (run-engine-parallel '(gamma 1.2) :relativity 20000 num-threads)

          (format t "~%--- Running Thermodynamics Domain ---~%")
          (run-engine-parallel '(carnot 0 t_cold) :thermo 50000 num-threads)

          (format t "~%--- Running Holography Domain ---~%")
          (run-engine-parallel '(entropy volume) :holography 200 num-threads)

          (format t "~%--- Running Logic Domain ---~%")
          (run-engine-parallel '(and p (not p)) :logic 200 num-threads)

          (format t "~%--- Running Hardware Domain (Silicon Lowering) ---~%")
          (run-engine-parallel '(and p (or q r)) :hardware 200 num-threads)

          (format t "~%--- Running Grand Unified Cross-Domain ---~%")
          (run-engine-parallel '(add (seq (h q2) (cnot q2 q1)) (carnot 0 t_cold)) :unified 200 num-threads)))))

(unless (find-package :cognitive-engine-noauto)
  (main))
