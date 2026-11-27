#!/usr/bin/env bash
set -euo pipefail

package_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
cd "$package_dir/../.."
target_dir=${CARGO_TARGET_DIR:-target}
node "$package_dir/generate-sheetjs-options.mjs" --check
cargo build --release --target wasm32-unknown-unknown -p ooxml-xlsx-wasm
wasm-bindgen --target web --out-dir "$package_dir/wasm" \
    "$target_dir/wasm32-unknown-unknown/release/ooxml_xlsx_wasm.wasm"
node "$package_dir/build-cfb.mjs"
cp LICENSE-MIT LICENSE-APACHE "$package_dir/"
