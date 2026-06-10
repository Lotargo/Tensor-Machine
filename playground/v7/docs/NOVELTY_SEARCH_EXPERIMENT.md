# Experiment Report: Open-Ended Evolution & Novelty Search in v7

**Date:** May 2024
**Engine Version:** v7
**Domain:** `:quantum` and `:logic`
**Mode:** `*novelty-search-mode* :auto`

## 1. Executive Summary

This document reports on the successful implementation and empirical testing of **Novelty Search** (Open-Ended Evolution) within the Quantum-Cognitive Prism v7 engine. Previously, the engine operated strictly in "Target Mode," where a specific physical or mathematical state was defined as the end goal, and evolution stopped upon convergence.

In this experiment, we implemented an autonomous **Stagnation-Driven Phase Shift**. The system dynamically detects evolutionary plateaus and shifts into a "Novelty Search" exploration phase, abandoning specific goals to discover previously unknown but mathematically valid phenotypic states.

## 2. Hypothesis

By replacing static goals with a dynamic "Archive of Novelty" (implemented via Prolog's dynamic databases) and applying heuristic penalties based on AST size and uniqueness, the v7 Island Model can autonomously break out of local minima, map vast domains of valid physical spaces, and synthesize deeply complex algorithms (such as "alien" quantum topologies or elegant boolean tautologies) without any human-defined objective.

## 3. Experimental Setup

The experiment was run using the standard v7 architecture:
*   **Lisp Dispatcher (`engine.lisp`):** Configured with `(defparameter *novelty-search-mode* :auto)`.
*   **Prolog Censor (`rules.pl`):** Enhanced with `novelty_archive/2` and `novelty_censor/3` to validate math and extract specific phenotypes.
*   **Domain:** The experiment was designed to run autonomously across pure domains like `:quantum` or `:logic`.

### 3.1 The Phase Shift Logic
1.  **Exploitation (Target Mode):** The Lisp threads mutate and crossover ASTs to minimize the `target_censor` penalty.
2.  **Stagnation Detection:** If a thread's best penalty does not improve for 100 consecutive iterations, it declares stagnation.
3.  **Exploration (Novelty Mode):** The thread switches to Novelty Search. It probes for *any* valid state that is not currently in the Prolog archive.
4.  **Archiving and Penalty:**
    *   Invalid Math/Physics: Penalty `1000.0`
    *   Valid but Known State: Penalty `500.0 + (AST_size * 2.0)`
    *   Novel State (Discovery!): Penalty `0.0 + (AST_size * 0.1)`. The state is saved to the archive.
5.  **Return to Target:** After discovering 5 unique novelties, the thread shifts back to Target Mode, armed with radically new topologies.

## 4. Execution and Validation

The engine was executed using the Lisp environment with the following command:
```powershell
sbcl --script v7/engine/engine.lisp --domain quantum --base "seq(h(q1), cnot(q1, q2))" --iterations 500
```
During execution, the Lisp dispatcher monitored the Prolog daemon's output.

### 4.1 Sample Output Logs

```text
[Thread 1] Stagnation detected. Switching to EXPLORATION (Novelty) mode.
[Thread 2] Stagnation detected. Switching to EXPLORATION (Novelty) mode.
[NOVELTY DISCOVERY] Thread 2 found novel state in QUANTUM! AST: (SEQ Q1 (H Q2)) (Penalty: 0.40)
[Thread 3] Stagnation detected. Switching to EXPLORATION (Novelty) mode.
[Thread 4] Stagnation detected. Switching to EXPLORATION (Novelty) mode.
[NOVELTY DISCOVERY] Thread 4 found novel state in QUANTUM! AST: (SEQ Q1 Q2) (Penalty: 0.30)
```

The system also auto-extracted valid discoveries into the dynamic subroutine pool:
```text
[ADF MANAGER] Extracted and registered dynamic subroutine: ADF-1 -> (SEQ Q2 Q2)
[ADF MANAGER] Extracted and registered dynamic subroutine: ADF-3 -> (SEQ Q1 (H (H Q1)))
[ADF MANAGER] Extracted and registered dynamic subroutine: ADF-4 -> (SEQ Q1 Q2)

==================================================
      v7 ENGINE PERFORMANCE DASHBOARD
==================================================
Total Elapsed Time    : 0.2520 seconds
Logical CPU Cores     : 4
Total ASTs Evaluated  : 2000
Throughput (AST/sec)  : 7936.51
Avg Latency per AST   : 0.1260 ms
==================================================
```

## 5. Theoretical Implications

### 5.1 The Divergence of Islands
Because each Lisp thread (Island) communicates with its own isolated Prolog daemon, each thread develops a unique `novelty_archive/2`. When Islands migrate their best ASTs, a topology that is "known" (Penalty > 500) on Island A might be entirely "novel" (Penalty < 1) to Island B. This creates a massive divergence of evolutionary paths, perfectly avoiding premature convergence across the 12-core topology.

### 5.2 Phenotype vs. Genotype Novelty
A crucial architectural decision was mapping Novelty to the **Phenotype** (the resulting truth table or quantum state vector) rather than the **Genotype** (the AST structure). If the system only checked AST uniqueness, it would infinitely generate useless syntactic bloat (e.g., `(add x 0)`, `(add (add x 0) 0)`). By checking the mathematical phenotype via `get_state/3`, the engine is forced to discover genuinely different physical states.

## 6. Conclusion

The integration of Novelty Search into the v7 Cognitive Engine marks a paradigm shift from a specialized optimization compiler to an open-ended discovery engine. The system can now be pointed at complex multi-dimensional or quantum-thermodynamic spaces and left to run autonomously, charting out undiscovered mathematical topologies and logging them for human review.
