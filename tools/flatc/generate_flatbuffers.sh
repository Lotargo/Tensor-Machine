#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/../.." && pwd)"
flatc="$("${script_dir}/resolve_flatc.sh")"

cd "${repo_root}"
"${flatc}" --rust -o tensor-rag/src tensor-rag/src/result.fbs
"${flatc}" --rust -o tensor-rag/src tensor-rag/src/parser_protocol.fbs
mkdir -p parser-sidecar/generated
"${flatc}" --python -o parser-sidecar/generated tensor-rag/src/parser_protocol.fbs
