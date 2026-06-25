#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
version="25.12.19"

if [[ -n "${FLATC:-}" && -x "${FLATC}" ]]; then
  printf '%s\n' "${FLATC}"
  exit 0
fi

if command -v flatc >/dev/null 2>&1; then
  command -v flatc
  exit 0
fi

os="$(uname -s)"
arch="$(uname -m)"

case "${os}:${arch}" in
  Linux:x86_64|Linux:amd64)
    archive_name="linux-x86_64-gcc13.zip"
    platform="linux-x86_64-gcc13"
    binary_name="flatc"
    ;;
  Darwin:arm64|Darwin:aarch64)
    archive_name="macos-arm64.zip"
    platform="macos-arm64"
    binary_name="flatc"
    ;;
  Darwin:x86_64|Darwin:amd64)
    archive_name="macos-x86_64.zip"
    platform="macos-x86_64"
    binary_name="flatc"
    ;;
  *)
    printf 'Unsupported platform for vendored flatc: %s/%s\n' "${os}" "${arch}" >&2
    exit 1
    ;;
esac

archive="${script_dir}/v${version}/${archive_name}"
cache_dir="${script_dir}/.cache/v${version}/${platform}"
cached_flatc="${cache_dir}/${binary_name}"

if [[ ! -x "${cached_flatc}" ]]; then
  if [[ ! -f "${archive}" ]]; then
    printf 'Vendored flatc archive not found: %s\n' "${archive}" >&2
    exit 1
  fi
  mkdir -p "${cache_dir}"
  unzip -oq "${archive}" -d "${cache_dir}"
  found="$(find "${cache_dir}" -type f -name "${binary_name}" | head -n 1)"
  if [[ -z "${found}" ]]; then
    printf 'flatc was not found after extracting %s\n' "${archive}" >&2
    exit 1
  fi
  chmod +x "${found}"
  if [[ "${found}" != "${cached_flatc}" ]]; then
    cp "${found}" "${cached_flatc}"
    chmod +x "${cached_flatc}"
  fi
fi

printf '%s\n' "${cached_flatc}"
