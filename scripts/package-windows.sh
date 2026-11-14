#!/usr/bin/env bash
# Compila o SpiderDesk em release para Windows (cross-compile a partir do Linux, alvo GNU)
# e monta uma pasta distribuivel em dist/spiderdesk-windows/.
#
# Requisitos: rustup target add x86_64-pc-windows-gnu ; mingw-w64 (x86_64-w64-mingw32-gcc).
# No proprio Windows basta:  cargo build --release   e copiar config/ e assets/ ao lado do .exe
set -euo pipefail
cd "$(dirname "$0")/.."

TARGET=x86_64-pc-windows-gnu
cargo build --release --target "$TARGET"

OUT=dist/spiderdesk-windows
rm -rf "$OUT"
mkdir -p "$OUT/config" "$OUT/assets"
cp "${CARGO_TARGET_DIR:-target}/$TARGET/release/spiderdesk.exe" "$OUT/"
cp config/default.toml "$OUT/config/"
cp -r config/models "$OUT/config/models"
cp -r assets/. "$OUT/assets/"
cp README.md docs/TESTES_WINDOWS.md "$OUT/"
echo "pronto: $OUT"
ls -la "$OUT"
