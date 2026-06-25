# Vendored flatc

This directory vendors the small official `flatc` release archives used to
generate Rust and Python FlatBuffers bindings for the parser sidecar protocol.

Version: `25.12.19`

Resolution order:

1. `FLATC` environment variable, when set.
2. `flatc` from `PATH`.
3. Vendored archive for the current OS/architecture.

The vendored archives are intentionally committed to keep code generation
available offline and consistent across Windows, Linux, and macOS.

All official release archives currently vendored here are below 3 MB each, so
Git LFS is not required.

The extracted binaries live under `tools/flatc/.cache/` and are ignored by git.

## Commands

Windows:

```powershell
powershell -ExecutionPolicy Bypass -File tools\flatc\resolve_flatc.ps1
powershell -ExecutionPolicy Bypass -File tools\flatc\generate_flatbuffers.ps1
```

Linux/macOS:

```bash
bash tools/flatc/resolve_flatc.sh
bash tools/flatc/generate_flatbuffers.sh
```
