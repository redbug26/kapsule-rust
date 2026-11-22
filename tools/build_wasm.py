#!/usr/bin/env python3
"""Build and seal an allowlisted static WASM distribution."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
from sync_store import sync_store
from web_distribution import STATIC_FILES, seal

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--out-dir", type=Path, default=ROOT / "dist/web")
parser.add_argument("--target-dir", type=Path)
args = parser.parse_args()
if args.out_dir.is_symlink():
    raise SystemExit("Output must not be a symlink")
out = args.out_dir.resolve()
if out.exists() and any(out.iterdir()) and not (out / "build-manifest.json").is_file():
    legacy = out == ROOT / "dist/web" and (out / "kapsule.wasm").is_file()
    if not legacy or any(item.name not in STATIC_FILES | {"store", "build-manifest.json"} for item in out.iterdir()):
        raise SystemExit("Refusing to replace a non-generated output directory")
if out == ROOT or out in ROOT.parents:
    raise SystemExit("Output directory must not contain the source repository")
if not shutil.which("emcc"):
    raise SystemExit("Activate an Emscripten SDK before building: source /path/to/emsdk/emsdk_env.sh")
toolchain = os.environ.get("KAPSULE_RUST_TOOLCHAIN", "stable")
command = ["rustup", "run", toolchain, "cargo"]
env = os.environ.copy()
env["RUSTC"] = subprocess.check_output(["rustup", "which", "--toolchain", toolchain, "rustc"], text=True).strip()
build = command + ["build", "--release", "--locked", "--target", "wasm32-unknown-emscripten", "--bin", "kapsule"]
if args.target_dir:
    target = args.target_dir.resolve()
    build += ["--target-dir", str(target)]
else:
    metadata = json.loads(subprocess.check_output(command + ["metadata", "--no-deps", "--format-version", "1"], cwd=ROOT, env=env))
    target = Path(metadata["target_directory"])
subprocess.run(build, cwd=ROOT, env=env, check=True)
artifacts = target / "wasm32-unknown-emscripten/release"
out.parent.mkdir(parents=True, exist_ok=True)
with tempfile.TemporaryDirectory(prefix="kapsule-web-", dir=out.parent) as temporary:
    stage = Path(temporary) / "web"
    stage.mkdir()
    for name in STATIC_FILES - {"demos.json", "kapsule.js", "kapsule.wasm"}:
        source = ROOT / "web" / name
        if source.is_symlink():
            raise SystemExit(f"Refusing symlinked public asset: {name}")
        shutil.copy2(source, stage / name)
    for name in ["kapsule.js", "kapsule.wasm"]:
        shutil.copy2(artifacts / name, stage / name)
    hello = ROOT / "examples/hello"
    demos = {"hello": {str(p.relative_to(hello)): p.read_text(encoding="utf-8") for p in hello.rglob("*") if p.suffix in (".lua", ".toml")}}
    (stage / "demos.json").write_text(json.dumps(demos, ensure_ascii=False), encoding="utf-8")
    sync_store(stage / "store", cache=out / "store")
    # Temporary directories and cloud-synced sources may have private modes.
    # The published static tree must remain readable by the HTTP server.
    for item in [stage, *stage.rglob("*")]:
        item.chmod(0o755 if item.is_dir() else 0o644)
    seal(stage)
    backup = Path(temporary) / "previous"
    if out.exists():
        if out.is_symlink():
            raise SystemExit("Output must not be a symlink")
        out.rename(backup)
    try:
        stage.rename(out)
    except BaseException:
        if backup.exists():
            backup.rename(out)
        raise
print(f"Browser distribution: {out}\nPreview: python3 tools/serve_web.py --port 8080")
