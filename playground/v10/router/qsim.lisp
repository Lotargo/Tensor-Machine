(defpackage :tcbp-qsim
  (:use :cl)
  (:export :apply-gate))

(in-package :tcbp-qsim)

;; Extremely naive quantum simulator for 1 or 2 qubits
(defparameter *inv-sqrt-2* (coerce (/ 1.0 (sqrt 2.0)) 'double-float))

(defun apply-hadamard (state-complex target-qubit num-qubits)
  (let* ((num-states (ash 1 num-qubits))
         (new-state (make-array num-states :element-type '(complex double-float) :initial-element #C(0.0d0 0.0d0))))
    (dotimes (i num-states)
      (let* ((bit-val (logand (ash i (- target-qubit)) 1))
             (partner (logxor i (ash 1 target-qubit))))
        (if (= bit-val 0)
            (setf (aref new-state i) (* *inv-sqrt-2* (+ (aref state-complex i) (aref state-complex partner))))
            (setf (aref new-state i) (* *inv-sqrt-2* (- (aref state-complex partner) (aref state-complex i)))))))
    new-state))

(defun apply-cnot (state-complex control-qubit target-qubit num-qubits)
  (let* ((num-states (ash 1 num-qubits))
         (new-state (make-array num-states :element-type '(complex double-float) :initial-element #C(0.0d0 0.0d0))))
    (dotimes (i num-states)
      (let ((c-bit (logand (ash i (- control-qubit)) 1)))
        (if (= c-bit 1)
            (let ((partner (logxor i (ash 1 target-qubit))))
              (setf (aref new-state i) (aref state-complex partner)))
            (setf (aref new-state i) (aref state-complex i)))))
    new-state))

(defun apply-gate (gate-type state-complex control-q target-q num-qubits)
  (cond
    ((string= gate-type "H") (apply-hadamard state-complex target-q num-qubits))
    ((string= gate-type "CNOT") (apply-cnot state-complex control-q target-q num-qubits))
    (t (error "Unsupported gate: ~A" gate-type))))
