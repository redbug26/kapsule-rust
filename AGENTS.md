# Kapsule reference runtime

This standalone Rust application
(https://github.com/redbug26/kapsule-rust) is the source of truth for Kapsule execution:
`apps.toml`, Lua 5.4 callbacks and the nine `kkc-*` modules. Keep the historical
Lua names/signatures compatible with existing store applications.

Developer documentation is maintained in
[development guide](https://github.com/redbug26/kkc-store/blob/main/docs/lua-applications.md)
and [API reference](https://github.com/redbug26/kkc-store/blob/main/docs/lua-api.md). Update those documents for
contract changes, rather than establishing a competing API reference here.

Consumer hosts must follow this runtime: `../../kkc/kkc-rust`, `../../ios/kkc`
and `../../ios/commando`. Keep platform limitations explicit, without silently
removing module members. Preserve terminal restoration on all error paths.

Run `cargo fmt --check`, `cargo test --locked`,
`cargo clippy --all-targets --locked -- -D warnings`, and
`python3 tools/test_terminal.py` for runtime changes. Fixtures in
`tests/fixtures` originate from kkc-store; keep their Lua and manifests unchanged
when testing compatibility. Do not require sibling repositories to build or test.

## WebAssembly host

The browser target is `wasm32-unknown-emscripten`, with Emscripten 4.0.23 and
Rust 1.95.0 tested. Keep common Lua loading, manifest parsing, drawing and
module behavior in `src/runtime.rs`. `src/web.rs` is the browser event/C ABI
host; `src/audio_web.rs` and `src/dialog_web.rs` adapt platform services.
The frontend is in `web/`; `tools/build_wasm.py` packages `dist/web`.

Use an activated Emscripten SDK, run `python3 tools/build_wasm.py`, then
`node tools/test_wasm.mjs`. Verify browser rendering and keyboard interaction
when changing the frontend. Web files are temporary MEMFS data; shell processes
must return an explicit unsupported error. Preserve real key-up handling and
focus-loss clearing; do not generate synthetic keyboard repeats.


The public player URL is https://kapsule.kyuran.be. `web/store.js` discovers all
Lua Kapsules through the store index and a pinned GitHub commit, with complete
binary-safe application directories. `tools/sync_store.py` bundles the same
catalog and files for offline/rate-limit fallback. Keep the descriptor path
independent of application ID, preserve download cancellation and size limits,
and validate expected IDs through the Rust manifest loader.

For store/publication changes run `node tools/test_store.mjs`,
`node tools/test_wasm.mjs` after building, `bash -n publish.sh`, and
`python3 tools/test_publish.py`. The publication test mocks rsync and makes no
SSH connection. `publish.sh` is an explicit user-invoked deployment operation;
creating or testing it does not authorize publishing the site.
