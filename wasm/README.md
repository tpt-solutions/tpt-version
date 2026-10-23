# tpt-version in the browser

A small demo: a key-value document with commits, branches, merges and history,
running entirely in the page. The core is `no_std`, so the same code runs on
WASM with no server.

## Build

```sh
rustup target add wasm32-unknown-unknown
cargo build -p tpt-version-wasm --target wasm32-unknown-unknown --release

# The wasm-bindgen CLI must match the wasm-bindgen version in Cargo.lock.
cargo install wasm-bindgen-cli --version <version from Cargo.lock> --locked
wasm-bindgen --target web --out-dir wasm/www/pkg \
  target/wasm32-unknown-unknown/release/tpt_version_wasm.wasm
```

## Run

Browsers refuse to load modules from `file://`, so serve the folder:

```sh
python -m http.server -d wasm/www 8000
```

Then open <http://localhost:8000>.
