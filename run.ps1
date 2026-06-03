# Clear host and set console settings
Clear-Host

$banner = @"
  _______                               __  ___           __  _
 /_  __/__  ____  _________  _____     /  |/  /___ ______/ /_  (_)___  ___
  / / / _ \/ __ \/ ___/ __ \/ ___/    / /|_/ / __ `/ ___/ __ \/ / __ \/ _ \
 / / /  __/ / / (__  ) /_/ / /       / /  / / /_/ / /__/ / / / / / / /  __/
/_/  \___/_/ /_/____/\____/_/       /_/  /_/\__,_/\___/_/ /_/_/_/ /_/\___/
                                                     Low-Latency RAG Engine
"@

function Show-Menu {
    Clear-Host
    Write-Host $banner -ForegroundColor Cyan
    Write-Host ""
    Write-Host "  1. Install (Cargo Build)" -ForegroundColor Gray
    Write-Host "  2. Start Server (Cargo Run)" -ForegroundColor Gray
    Write-Host "  3. Run Tests (Cargo Test)" -ForegroundColor Gray
    Write-Host "  4. Exit" -ForegroundColor Gray
    Write-Host ""
    Write-Host "Enter your choice [1-4]: " -NoNewline -ForegroundColor White
}

$running = $true
while ($running) {
    Show-Menu
    $choice = Read-Host

    switch ($choice) {
        "1" {
            Clear-Host
            Write-Host "=== Starting Project Build ===" -ForegroundColor Cyan
            cargo build --manifest-path tensor-rag/Cargo.toml --ignore-rust-version
            Write-Host "`nPress [Enter] to return to menu..." -ForegroundColor DarkGray
            Read-Host
        }
        "2" {
            Clear-Host
            Write-Host "=== Starting RAG Server ===" -ForegroundColor Cyan
            $process = Start-Process cargo -ArgumentList "run --manifest-path tensor-rag/Cargo.toml --bin tensor-rag --ignore-rust-version" -PassThru -NoNewWindow
            Write-Host "`nServer is running in background (PID: $($process.Id))." -ForegroundColor Green
            Write-Host "Press [Enter] key to stop the server..." -ForegroundColor Yellow
            Read-Host
            Stop-Process -Id $process.Id -Force
            Write-Host "Server stopped." -ForegroundColor Red
            Write-Host "`nPress [Enter] to return to menu..." -ForegroundColor DarkGray
            Read-Host
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
