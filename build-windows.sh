#!/bin/sh
# Build the Windows release from macOS/Linux (needs mingw-w64 + the
# x86_64-pc-windows-gnu Rust target). Output: dist/NotepadMD+.exe
set -e
cd "$(dirname "$0")"
# keep local filesystem paths (usernames etc.) out of the shipped binary
export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }--remap-path-prefix=$HOME=/build --remap-path-prefix=$PWD=/src"
cargo build --release --target x86_64-pc-windows-gnu
mkdir -p dist
cp target/x86_64-pc-windows-gnu/release/notepadmd_plus.exe "dist/NotepadMD+.exe"
echo "Built: dist/NotepadMD+.exe"
