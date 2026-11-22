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
