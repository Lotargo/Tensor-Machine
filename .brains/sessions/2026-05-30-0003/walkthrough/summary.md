# Session 2026-05-30-0003: Extract I/O operations from VM

## Objective
Task 5.1.1: Extract I/O operations (network requests to Qdrant/Postgres) out of the VM instructions. The VM should accept ready-made data instead of downloading it itself.

## Actions Taken
- Created an asynchronous `execute_program` orchestrator in `mcp.rs` which traverses the intermediate representation (IR) bytecode.
- The `execute_program` orchestrator performs the necessary network I/O (`DbPools::search_qdrant` and `DbPools::fetch_bm25_scores`) and supplies the data to the VM directly via `vm.batch` and `vm.stack`.
- Updated `vm.rs` to expose a `vm.step(opcode)` method that exclusively executes pure math operations, removing the execution of `RetrieveQdrant` and `RetrievePostgres` from the core VM logic loop.
- Adjusted `ToolHandler` to be async so the entire pipeline handles async I/O seamlessly without blocking the current thread.

## Outcomes
- The Virtual Machine logic strictly evaluates mathematical tensor operations.
- The network calls are extracted entirely to the `execute_program` orchestrator, honoring the Phase 5 technical debt requirement.
- All tests pass cleanly.
