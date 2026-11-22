# Security and pre-publication review

Reviewed on 2026-11-22. This is a source and local runtime review, not a
penetration test of the deployed Alwaysdata host.

## Trust boundaries

The native player executes trusted Lua with the current user's filesystem
and process permissions. It is not a sandbox. Do not run unknown Kapsules
with the native player.

The browser copies application files into an ephemeral Emscripten filesystem.
It cannot modify the imported local folder. Each application gets a fresh
worker; stopping or replacing it destroys that instance. Files are not saved
between sessions. Shell commands are explicitly unsupported.

Lua allocations are capped at 64 MiB; WASM memory is capped at 512 MiB. A
callback unresponsive for five seconds terminates its worker. Browser modals
pause this watchdog; dialog count and response size are bounded. These limits
mitigate broken applications, but do not guarantee isolation against every
malicious Lua program or a vulnerability in Lua, Emscripten, or the browser.
The browser UI still asks users to run trusted applications.

## Protections checked

- Store metadata and files use one pinned GitHub commit. Manifests must match
  the selected application ID. Downloads reject traversal, symlinks, duplicate
  paths, incomplete files and application size/count excesses.
- Bundled resources are base64 members of JSON packages. A Kapsule's `.php` or
  `.htaccess` remains data and cannot become server code or configuration.
- Publication builds a new distribution from an explicit asset allowlist.
  Validation rejects extra files, symlinks and changes to its SHA-256 manifest.
  The manifest detects accidental changes; it is not a cryptographic signature
  proving upstream publisher identity.
- WASM calls fixed JavaScript imports; dynamic JavaScript evaluation is
  disabled. Application labels and errors use text content rather than HTML.
- Apache configuration disables listing and CGI execution, forces static
  handling, and provides CSP, nosniff, frame restrictions and COOP/COEP headers.
  These headers must be permitted by the production host. HTTPS and cross-origin
  isolation are required for synchronous browser dialogs.
- Every publication rebuilds WASM and checks the latest store revision before
  rsync. A dedicated temporary directory contains compilation and staging
  output, removed on success, failure and handled interruptions. Shared Cargo
  dependency downloads and existing development builds are retained. SIGKILL
  and machine crashes cannot run shell cleanup traps.
- Published directories and files use readable static-site permissions.
  SSH authentication uses the existing `alwaysdata` alias, without embedded
  credentials. Only the sealed static distribution is transferred.

## Verification

The native format check, eight Rust tests, Clippy and terminal PTY checks
passed. The WASM build and WASM Clippy passed. Store tests cover pinned
fetches, binary resources, bundled fallback, limits and cancellation.

Real WASM tests loaded seven browser-compatible store Kapsules and verified
an explicit shell-process rejection for the eighth. Worker tests cover
synchronous dialogs, infinite startup/update loops, cancellation and recovery.
Distribution tests reject unexpected files, symlinks and tampering and verify
that PHP and Apache configuration stay inert. Publication tests mock rsync:
rebuilding, validation, dry runs, failures, interruption and cleanup require no
SSH connection. An end-to-end check also rebuilt the real WASM binary and
refreshed the live store with rsync replaced by a local probe; static file
permissions and removal of the temporary build directory passed.

Safari on localhost was checked with the publication headers: the catalog,
Rockland rendering, keyboard input and Stop work. Production TLS, Apache
permissions and response headers must still be checked after deployment.

`cargo audit` 0.23.0 reported zero known vulnerabilities and no warnings for
288 locked dependencies, using RustSec database commit
`d1fe6ac3eba823a3515e849045f36dc34543885d`. This result is time-dependent and
does not audit the external SDK, browser, host or each store application's code.

Run the commands in [README.md](README.md) after changes. Report security
issues through the repository owner's private contact channel; avoid posting
credentials or sensitive exploit data in public issues.
