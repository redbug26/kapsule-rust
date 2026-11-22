# Kapsule

A standalone Rust player and the **reference implementation for Kapsules**:
Lua 5.4 applications used by KKC Rust, KKC for iPad, and Kommando for iPad.
Lua is embedded: running the player does not require a system Lua interpreter
or another KKC repository.

- Browser player: [kapsule.kyuran.be](https://kapsule.kyuran.be/)
- Source: [redbug26/kapsule-rust](https://github.com/redbug26/kapsule-rust)
- Developer guide: [kkc-store/docs/lua-applications.md](https://github.com/redbug26/kkc-store/blob/main/docs/lua-applications.md)
- Lua API: [kkc-store/docs/lua-api.md](https://github.com/redbug26/kkc-store/blob/main/docs/lua-api.md)

## Build and run the native player

Install a stable Rust toolchain, then run:

```sh
cargo build --release --locked --target-dir target
./target/release/kapsule examples/hello
./target/release/kapsule /path/to/kapsule/apps.toml -- argument1 argument2
```

Alternatively, install the command with `cargo install --path . --locked`.
On Debian/Ubuntu, audio compilation requires `libasound2-dev`. The compiled
binary uses the platform's system audio libraries, but does not depend on
any other KKC project.

Pass either a directory containing `apps.toml` or the manifest itself. The
player loads the original application files without installing a separate
copy. Application code can modify files through the APIs described below.
Sibling Lua modules are available through `require`.

Relative paths in `kkc-fs` resolve from the Kapsule directory. `kkc.get_cwd()`
and the default `kkc-shell` working directory use the launch directory;
`--cwd DIRECTORY` overrides it.

**Run only trusted Kapsules in the native player.** File access and processes
have your user account's permissions. The native runtime is not a sandbox.

The player starts zoomed. **F5** switches to the manifest's requested window
size; `--windowed` starts in that window. **Ctrl-C** exits the player, including
from a dialog. **Esc** is delivered to the application. The terminal is
restored on exit and after Lua errors.

### Keyboard support

The player supports keyboard events, mouse input, focus changes, RGB colors,
and application shortcut bars. A terminal reporting real **key-up** events
provides more reliable held-key input for games. macOS Terminal does not
report releases. A startup message explains this limitation when release
support cannot be enabled; Enter or Esc dismisses it.

With negotiated key-up support, `is_down` remains true until release or focus
loss, including the operating-system delay before the first repeat. Without
it, the historical 200 ms last-event timeout approximates held keys. The
player does not generate keyboard repeats.

In iTerm, the profile must allow **Settings > Profiles > Keys > General >
Apps can change how keys are reported**. See the
[iTerm documentation](https://iterm2.com/documentation-one-page.html#keys-profile-preferences).

## Validate a Kapsule

```sh
kapsule --check /path/to/kapsule
kapsule --headless 60 --dump-screen /path/to/kapsule -- argument1
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
python3 tools/test_terminal.py
```

`--check` validates the manifest, entry path, and Lua syntax without executing
the application. `--headless N` calls `init`, `resize`, and up to N
`update`/`draw`/`shortcuts` cycles at a fixed `1/fps` timestep, using a virtual
80×24 terminal. It does not inject input or make wall-clock time and random
numbers deterministic. `--dump-screen` prints the final Lua buffer without
host borders or the shortcut bar. Interactive dialogs return an error in
headless mode.

Native compatibility tests include eight unmodified store snapshots. They
check loading and initial frames, modules, arguments, files, errors, and the
CLI. PTY tests exercise real terminal input and restoration. Fixtures are
compatibility data, not automatically installed applications.

## Reference behavior and compatibility

This repository defines the `apps.toml` manifest (`[application]` and
`[runtime]`), callbacks, and nine historical `kkc-*` modules. The original
engine came from `kkc-rust/src/lua_apps.rs`; Lua module names remain unchanged.

- `src/runtime.rs`: shared loading, Lua modules, graphics, and native loop.
- `src/lua_dialog.rs`: native dialogs, searchable selection, and checkboxes.
- `src/audio.rs`: native Rodio/Symphonia audio playback.
- `src/web.rs`: browser host and C ABI.
- `web/`: browser interface and store loader.
- `tests/compatibility.rs` and `tools/test_terminal.py`: native checks.

Native audio supports Symphonia formats, including WAV, MP3, FLAC, and Ogg.
KKC tracker decoders and native plugins are not included; unsupported formats
raise a Lua error. Historical dialog palette names use one beige player style.
Native mouse coordinates remain 1-based terminal-global coordinates, as in
the existing contract.

Document contract changes in **kkc-store**, add compatibility coverage, and
synchronize the consumer hosts: KKC Rust, KKC iPad, and Kommando iPad.
Platform adaptations preserve module members and signatures and report
unsupported operations explicitly.

## WebAssembly player

The browser player uses the same Rust/Lua 5.4 engine through the
`wasm32-unknown-emscripten` target. It provides a character-cell canvas, RGB
colors, real keyboard press/release events, mouse input, focus, arguments,
F5 zoom, and touch controls.

The menu discovers all Lua Kapsules in the
[KKC store](https://github.com/redbug26/kkc-store), including their descriptions
and versions. **Actualiser le store** refreshes the catalog. **Lancer** loads
all modules and assets from the selected application's directory. **Annuler**
cancels downloading. **Ouvrir un dossier** imports a personal Kapsule. The
local Hello example remains available without a network connection.

Install the Rust target and [Emscripten SDK](https://emscripten.org/docs/getting_started/downloads.html).
The tested combination is **Rust 1.95.0** and **Emscripten 4.0.23**:

```sh
rustup target add --toolchain stable wasm32-unknown-emscripten
source /path/to/emsdk/emsdk_env.sh
python3 tools/build_wasm.py
python3 tools/serve_web.py --port 8080
```

Open `http://localhost:8080`. The build refreshes the bundled store snapshot.
`dist/web` contains the complete static distribution, including the WASM
binary and JavaScript loader. No application server, account, or GitHub token
is required. Use an HTTP(S) server; opening `index.html` through `file://`
is not supported. The development server applies the same security headers
as the published site.

The build script uses Cargo and rustc from the same rustup toolchain to avoid
mixing installations. `KAPSULE_RUST_TOOLCHAIN` selects a toolchain instead of
the default `stable`. WASM exceptions support Lua error propagation. Native
build commands remain available independently of Emscripten.

### Browser behavior and limits

- All nine `kkc-*` modules retain their names and signatures. Platform-specific
  services use browser adapters.
- `kkc-fs` uses temporary Emscripten memory files. Imported folders are copied;
  the originals are never modified. Changes are lost when another Kapsule is
  opened or the page reloads. Applications are limited to 64 MiB and 4096 files.
- Keyboard releases drive held-key state. Focus loss clears it. The player
  generates no synthetic repeats. Frames pause while the page is hidden;
  resumed frame deltas are capped at 100 ms.
- Mouse coordinates are 1-based and relative to canvas content. The HTML
  frame lies outside the drawing area.
- Lua runs in a dedicated worker, stopped if a callback takes over five
  seconds (excluding browser modals). Lua allocations are limited to 64 MiB
  and the WASM heap to 512 MiB.
- Dialogs require HTTPS or localhost with COOP/COEP headers and use browser modals. `select` asks for an option number;
  `select_with_checks` then asks each checkbox state. Theme names are accepted
  with the browser's appearance.
- Beeps use Web Audio. File playback depends on browser codecs and interaction
  requirements. Asynchronous playback failures appear in the status area.
- `kkc-shell.run` raises an unsupported-operation error. Applications requiring
  Git or other local processes need the native player.
- Run only Kapsules you trust. Browser process isolation and resource limits
  reduce the impact of broken applications; they do not certify application
  code or make the native player safe for untrusted code.

### Store and launch links

The catalog comes from `dist/store-index.json` in `redbug26/kkc-store`. Only
entries with a `runtime` section are Kapsules; external applications and
plugins are excluded. The loader uses each entry's `descriptor` path, which
can differ from its ID (`my_lua_app` lives in `applications/template_lua_app`).
Rust validates that the downloaded manifest matches the selected ID.

The index, file listing, and downloads use one pinned GitHub commit. All
application files, including binary assets, are loaded with progress and
cancellation. Symlinks are not supported.

If GitHub is unavailable or rate-limited, the complete store snapshot from
the last build remains available. `tools/sync_store.py` creates this snapshot
without a local store checkout. Browser limitations still apply to bundled
applications. Share a selection with
[Rockland](https://kapsule.kyuran.be/?kapsule=rockland) or
[Calendar](https://kapsule.kyuran.be/?kapsule=calendar); the visitor clicks
**Lancer** to run it.

### Check the browser distribution

```sh
node tools/test_store.mjs
node tools/test_wasm.mjs
node tools/test_worker.mjs
python3 tools/test_security.py
python3 tools/test_publish.py
```

Tests exercise the real WASM binary, bundled applications, Lua modules,
virtual files, resizing, focus, and a held key followed by release. They also
check explicit process rejection, store loading, and publication preflight.

## Publish to Alwaysdata

The root `publish.sh` follows Kotv's SSH/rsync convention and targets:
`kardkiss@alwaysdata:/home/kardkiss/kapsule.kyuran.be/`.
Configure the Alwaysdata site to serve that directory over HTTPS at
`https://kapsule.kyuran.be`.

```sh
./publish.sh --dry-run  # Preview transfers; requires SSH access
./publish.sh            # Rebuild WASM, refresh the store, validate, then publish
```

Every invocation requires an installed Emscripten SDK, including `--dry-run`.
The script activates it automatically when `emcc` is absent from `PATH`. It
checks `EMSDK`, `~/.emsdk`, `~/emsdk` and the temporary SDK used during setup.
Set `KAPSULE_EMSDK=/path/to/emsdk` to select an installation explicitly.
The script always rebuilds WASM and refreshes the store from GitHub before
uploading. Compilation output and the staged website use a dedicated temporary
directory, removed on success, failure, or interruption. Existing development
builds and shared dependency caches are preserved. `--build` remains accepted
for compatibility. Preflight rejects incomplete,
modified, or unexpected distribution files before rsync runs. Publication
removes old remote files to match the local static distribution. Authentication
uses the existing `alwaysdata` SSH alias; no secret is embedded in the script.
The Apache configuration provides WASM MIME types, compression, cache
revalidation, and browser security headers.

See [SECURITY.md](SECURITY.md) for the security model, review scope, remaining
limitations, and pre-publication verification results.
