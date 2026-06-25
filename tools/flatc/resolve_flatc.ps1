$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$version = "25.12.19"
$archive = Join-Path $PSScriptRoot "v$version\windows-x86_64.zip"
$cacheDir = Join-Path $PSScriptRoot ".cache\v$version\windows-x86_64"
$cachedFlatc = Join-Path $cacheDir "flatc.exe"

if ($env:FLATC) {
    $candidate = Resolve-Path $env:FLATC -ErrorAction SilentlyContinue
    if ($candidate) {
        Write-Output $candidate.Path
        exit 0
    }
}

$pathFlatc = Get-Command flatc -ErrorAction SilentlyContinue
if ($pathFlatc) {
    Write-Output $pathFlatc.Source
    exit 0
}

if (!(Test-Path $cachedFlatc)) {
    if (!(Test-Path $archive)) {
        throw "Vendored flatc archive not found: $archive"
    }
    New-Item -ItemType Directory -Force $cacheDir | Out-Null
    try {
        Expand-Archive -Path $archive -DestinationPath $cacheDir -Force
    }
    catch {
        if (!(Test-Path $cachedFlatc)) {
            throw
        }
    }
}

if (!(Test-Path $cachedFlatc)) {
    $found = Get-ChildItem -Path $cacheDir -Recurse -Filter flatc.exe | Select-Object -First 1
    if (!$found) {
        throw "flatc.exe was not found after extracting $archive"
    }
    Write-Output $found.FullName
    exit 0
}

Write-Output (Resolve-Path $cachedFlatc).Path
