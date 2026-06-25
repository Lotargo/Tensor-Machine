$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$flatc = & (Join-Path $PSScriptRoot "resolve_flatc.ps1")

Push-Location $repoRoot
try {
    & $flatc --rust -o tensor-rag\src tensor-rag\src\result.fbs
    & $flatc --rust -o tensor-rag\src tensor-rag\src\parser_protocol.fbs
    New-Item -ItemType Directory -Force parser-sidecar\generated | Out-Null
    & $flatc --python -o parser-sidecar\generated tensor-rag\src\parser_protocol.fbs
}
finally {
    Pop-Location
}
