#!/usr/bin/env bash
# Rebuild, refresh the store, validate, publish, and remove publication artifacts.
# Usage: ./publish.sh [--dry-run|-n]
set -euo pipefail
export PYTHONDONTWRITEBYTECODE=1
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"
REMOTE="kardkiss@alwaysdata:/home/kardkiss/kapsule.kyuran.be/"
DRY_RUN=false
for arg in "$@"; do
    case "$arg" in
        --dry-run|-n) DRY_RUN=true ;;
        --build) : ;; # Backward-compatible: rebuilding is now unconditional.
        --help|-h)
            echo "Usage: ./publish.sh [--dry-run|-n]"
            echo "Always rebuilds WASM and refreshes the store before syncing to $REMOTE"
            echo "Activates an installed Emscripten SDK automatically (override: KAPSULE_EMSDK)."
            echo "--dry-run previews rsync without uploading."
            echo "Publication-only build and staging files are removed on success, failure or interruption."
            exit 0 ;;
        *) echo "Unknown option: $arg" >&2; exit 2 ;;
    esac
done
for command in python3 rsync; do
    command -v "$command" >/dev/null || { echo "Missing required command: $command" >&2; exit 1; }
done
# Environment activation is local to this process; the caller's shell is unchanged.
if [[ -n "${KAPSULE_EMSDK:-}" ]] || ! command -v emcc >/dev/null 2>&1; then
    if [[ -n "${KAPSULE_EMSDK:-}" ]]; then
        SDK_CANDIDATES=("$KAPSULE_EMSDK")
    else
        SDK_CANDIDATES=("${EMSDK:-}" "$HOME/.emsdk" "$HOME/emsdk" "${TMPDIR:-/tmp}/kapsule-emsdk" "/tmp/kapsule-emsdk")
    fi
    for sdk in "${SDK_CANDIDATES[@]}"; do
        if [[ -n "$sdk" && -f "$sdk/emsdk_env.sh" ]]; then
            printf 'Activating Emscripten SDK: %s\n' "$sdk"
            EMSDK_QUIET=1 source "$sdk/emsdk_env.sh" >/dev/null
            break
        fi
    done
    if ! command -v emcc >/dev/null 2>&1; then
        echo "Emscripten SDK not found or not installed. Set KAPSULE_EMSDK to an installed SDK directory containing emsdk_env.sh." >&2
        echo "Installation guide: https://emscripten.org/docs/getting_started/downloads.html" >&2
        exit 1
    fi
fi

STAGING="$(mktemp -d "${TMPDIR:-/tmp}/kapsule-publish.XXXXXX")"
cleanup() { rm -rf -- "$STAGING"; }
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

python3 "$ROOT/tools/build_wasm.py" --out-dir "$STAGING/web" --target-dir "$STAGING/target"
python3 "$ROOT/tools/web_distribution.py" "$STAGING/web"
RSYNC_ARGS=(-avz --delete --delay-updates -e ssh --exclude='.DS_Store' --exclude='._*')
if "$DRY_RUN"; then RSYNC_ARGS+=(--dry-run); fi
printf 'Publishing the rebuilt distribution to %s\n' "$REMOTE"
rsync "${RSYNC_ARGS[@]}" "$STAGING/web/" "$REMOTE"
echo "Publication finished; temporary distribution and compilation artifacts will be removed."
