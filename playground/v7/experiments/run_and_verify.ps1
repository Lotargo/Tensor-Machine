param(
    [int]$Iterations = 5000,
    [int]$Threads = 0
)

$ErrorActionPreference = "Stop"
$root = Resolve-Path "$PSScriptRoot\..\.."
$log = "$PSScriptRoot\toffoli_results.log"

if ($Threads -eq 0) {
    $Threads = [Environment]::ProcessorCount
}

Write-Host "=== Toffoli Decomposition Experiment ===" -ForegroundColor Cyan
Write-Host "Iterations per thread: $Iterations"
Write-Host "Threads: $Threads"
Write-Host "Log: $log"
Write-Host ""

# ── Step 1: Run the experiment ──
Write-Host ">>> Step 1: Running experiment..." -ForegroundColor Yellow
$sbcl = Get-Command "sbcl" -ErrorAction SilentlyContinue
if (-not $sbcl) {
    Write-Error "SBCL not found in PATH"
    exit 1
}

$exp_start = Get-Date
& $sbcl.Source --script "$PSScriptRoot\run_toffoli_decomp.lisp" 2>&1
if ($LASTEXITCODE -ne 0) {
    Write-Error "Experiment failed with exit code $LASTEXITCODE"
    exit 1
}
$exp_elapsed = [math]::Round(((Get-Date) - $exp_start).TotalSeconds, 2)
Write-Host "Experiment completed in ${exp_elapsed}s" -ForegroundColor Green

# ── Step 2: Verify ──
Write-Host ""
Write-Host ">>> Step 2: Verifying results..." -ForegroundColor Yellow
& python "$PSScriptRoot\verify_toffoli.py" $log 2>&1
if ($LASTEXITCODE -ne 0) {
    Write-Warning "Verifier finished with non-zero exit code"
}

# ── Step 3: Summary ──
Write-Host ""
Write-Host "=== Done ===" -ForegroundColor Cyan
Write-Host "Log: $log"
Write-Host "Total time: ${exp_elapsed}s"
