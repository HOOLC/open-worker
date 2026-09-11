#!/bin/sh
# Keep the app's Homebrew Rust/LLVM toolchain intact. Only the wasm linker
# resolves its newer LLVM/Z3 libraries from their own installed kegs.
set -eu
exec env DYLD_LIBRARY_PATH=/opt/homebrew/Cellar/llvm/23.1.0/lib:/opt/homebrew/Cellar/z3/5.1.0/lib /opt/homebrew/Cellar/lld/23.1.0/bin/wasm-ld "$@"
