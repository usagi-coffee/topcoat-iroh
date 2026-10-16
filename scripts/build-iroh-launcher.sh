#!/usr/bin/env bash
set -euo pipefail

# Run from the repository root. Publish the output at the root of an HTTPS origin.
output=${1:-target/iroh-launcher}
source_dir=crates/topcoat-runtime/iroh-browser/launcher
cargo build -p topcoat-runtime-iroh-browser --target wasm32-unknown-unknown --release --locked
mkdir -p "$output/__topcoat"
wasm-bindgen --target web --out-dir "$output/__topcoat" target/wasm32-unknown-unknown/release/topcoat_runtime_iroh_browser.wasm
cp "$source_dir"/{index.html,launcher.js,storage.js,transport.js} "$output/__topcoat/"
cp "$source_dir/topcoat-sw.js" "$output/"
cp "$source_dir/index.html" "$output/index.html"
cp "$source_dir/index.html" "$output/404.html"
if [[ ! -f "$output/__topcoat/config.json" ]]; then
    cp "$source_dir/config.json" "$output/__topcoat/config.json"
fi
touch "$output/.nojekyll"
