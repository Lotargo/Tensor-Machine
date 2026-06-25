# Clear host and set console settings
Clear-Host

$banner = @"
  _______                               __  ___           __ 
 /_  __/__  ____  _________  _____     /  |/  /___ ______/ /_  (_)___  ___
  / / / _ \/ __ \/ ___/ __ \/ ___/    / /|_/ / __ `/ ___/ __ \/ / __ \/ _ \
 / / /  __/ / / (__  ) /_/ / /       / /  / / /_/ / /__/ / / / / / / /  __/
/_/  \___/_/ /_/____/\____/_/       /_/  /_/\__,_/\___/_/ /_/_/_/ /_/\___/
                                                     Low-Latency RAG Engine
"@

function Initialize-Flatc {
    if ($env:FLATC -and (Test-Path $env:FLATC)) {
        return
    }

    $resolver = Join-Path $PSScriptRoot "tools\flatc\resolve_flatc.ps1"
    if (-not (Test-Path $resolver)) {
        Write-Host "flatc resolver not found at $resolver" -ForegroundColor Red
        exit 1
    }

    $env:FLATC = (& powershell -NoProfile -ExecutionPolicy Bypass -File $resolver).Trim()
    if ($LASTEXITCODE -ne 0 -or -not $env:FLATC) {
        Write-Host "Failed to resolve flatc." -ForegroundColor Red
        exit 1
    }
}

function Generate-Flatbuffers {
    $generator = Join-Path $PSScriptRoot "tools\flatc\generate_flatbuffers.ps1"
    if (-not (Test-Path $generator)) {
        Write-Host "FlatBuffers generator not found at $generator" -ForegroundColor Red
        exit 1
    }

    & powershell -NoProfile -ExecutionPolicy Bypass -File $generator
    if ($LASTEXITCODE -ne 0) {
        Write-Host "FlatBuffers generation failed." -ForegroundColor Red
        exit 1
    }
}

function Show-Menu {
    Clear-Host
    Write-Host $banner -ForegroundColor Cyan
    Write-Host ""
    Write-Host "  1. Install (Cargo Build)" -ForegroundColor Gray
    Write-Host "  2. Start Server (Build + Launch Binary)" -ForegroundColor Gray
    Write-Host "  3. Run Tests (Cargo Test)" -ForegroundColor Gray
    Write-Host "  4. Exit" -ForegroundColor Gray
    Write-Host ""
    Write-Host "Enter your choice [1-4]: " -NoNewline -ForegroundColor White
}

Initialize-Flatc

$running = $true
while ($running) {
    Show-Menu
    $choice = Read-Host

    switch ($choice) {
        "1" {
            Clear-Host
            Write-Host "=== Starting Project Build ===" -ForegroundColor Cyan
            Write-Host "=== Generating FlatBuffers Bindings ===" -ForegroundColor Cyan
            Generate-Flatbuffers
            cargo build --manifest-path tensor-rag/Cargo.toml --ignore-rust-version
            Write-Host "`nPress [Enter] to return to menu..." -ForegroundColor DarkGray
            Read-Host
        }
        "2" {
            Clear-Host
            Write-Host "=== Starting RAG Server ===" -ForegroundColor Cyan
            $env:TENSOR_RAG_LAUNCHER = "true"
            $restart = $true
            while ($restart) {
                cargo build --manifest-path tensor-rag/Cargo.toml --bin tensor-rag --ignore-rust-version
                if ($LASTEXITCODE -ne 0) {
                    Write-Host "`nBuild failed. Press [Enter] to return to menu..." -ForegroundColor Red
                    Read-Host
                    $restart = $false
                    continue
                }

                $serverPath = Join-Path $PSScriptRoot "tensor-rag\\target\\debug\\tensor-rag.exe"
                if (-not (Test-Path $serverPath)) {
                    Write-Host "`nBuilt binary not found at $serverPath" -ForegroundColor Red
                    Write-Host "Press [Enter] to return to menu..." -ForegroundColor DarkGray
                    Read-Host
                    $restart = $false
                    continue
                }

                Write-Host "Server is running in a separate window." -ForegroundColor Green
                Write-Host "To stop the server, close the server window or press [Enter] in this window." -ForegroundColor Cyan

                $process = Start-Process $serverPath -WorkingDirectory $PSScriptRoot -PassThru

                while (-not $process.HasExited) {
                    try {
                        if ([System.Console]::KeyAvailable) {
                            $key = [System.Console]::ReadKey($true)
                            if ($key.Key -eq 'Enter') {
                                Write-Host "`nStopping server..." -ForegroundColor Yellow
                                Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
                                break
                            }
                        }
                    } catch {
                        # Non-interactive environment or console not attached
                    }
                    Start-Sleep -Milliseconds 250
                }

                # Ensure process object has exited and capture exit code
                $process.WaitForExit()
                $exitCode = $process.ExitCode

                if ($exitCode -eq 123) {
                    Write-Host "`nServer requested restart (Exit Code 123). Rebuilding and restarting..." -ForegroundColor Cyan
                    # loop continues
                } else {
                    $restart = $false
                    Write-Host "`nServer stopped (Exit Code: $exitCode)." -ForegroundColor Red
                    Write-Host "Press [Enter] to return to menu..." -ForegroundColor DarkGray
                    Read-Host
                }
            }
        }
        "3" {
            Clear-Host
            Write-Host "=== Running Test Suite ===" -ForegroundColor Cyan
            cargo test --manifest-path tensor-rag/Cargo.toml --ignore-rust-version
            Write-Host "`nPress [Enter] to return to menu..." -ForegroundColor DarkGray
            Read-Host
        }
        "4" {
            Write-Host "Goodbye!" -ForegroundColor Yellow
            $running = $false
        }
        default {
            Write-Host "Invalid choice! Press [Enter] to try again..." -ForegroundColor Red
            Read-Host
        }
    }
}
