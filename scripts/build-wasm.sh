#!/usr/bin/env bash
# Build the catalog renderer for the browser and put it where the site imports it.
# Uses rustup's toolchain: a Homebrew rustc has no wasm32 standard library.
set -euo pipefail
cd "$(dirname "$0")/.."
if command -v rustup >/dev/null 2>&1; then
  PATH="$(dirname "$(rustup which cargo)"):$PATH"
  export PATH
  rustup target add wasm32-unknown-unknown >/dev/null 2>&1 || true
fi
cargo build -p twolab-tui-catalog-wasm --target wasm32-unknown-unknown --profile wasm --locked
cp target/wasm32-unknown-unknown/wasm/twolab_tui_catalog_wasm.wasm site/catalog.wasm
echo "site/catalog.wasm $(wc -c < site/catalog.wasm | tr -d ' ') bytes"
