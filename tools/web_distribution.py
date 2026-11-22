"""Strict public-distribution validation shared by build, tests and publication."""
import base64
import hashlib
import json
from pathlib import Path
import re

MAX_BYTES = 64 * 1024 * 1024
STATIC_FILES = frozenset(("index.html", "app.js", "engine-client.js", "engine-worker.js", "files.js", "store.js", "style.css", "kapsule.js", "kapsule.wasm", "demos.json", ".htaccess"))
MANIFEST = "build-manifest.json"
PACKAGE_RE = re.compile(r"packages/[0-9a-f]{64}\.json\Z")


def safe_path(value):
    if not isinstance(value, str) or not value or len(value) > 1024 or "\\" in value or "\0" in value or any(p in ("", ".", "..") for p in value.split("/")):
        raise ValueError("Invalid relative path")
    return value


def validate_store(root):
    root = Path(root)
    catalog = json.loads((root / "catalog.json").read_text(encoding="utf-8"))
    if catalog.get("format") != 2 or not re.fullmatch(r"[0-9a-f]{40}", catalog.get("commit", "")):
        raise ValueError("Invalid packaged-store format")
    apps, files, packages = catalog["applications"], catalog["files"], catalog["packages"]
    if not apps or not files or len({a["id"] for a in apps}) != len(apps) or set(packages) != {a["id"] for a in apps}:
        raise ValueError("Empty or ambiguous store")
    paths = set()
    for file in files:
        name = safe_path(file["path"])
        if name in paths or type(file["size"]) is not int or file["size"] < 0:
            raise ValueError("Invalid store file")
        paths.add(name)
    expected = {"catalog.json"}
    for app in apps:
        descriptor = safe_path(app["descriptor"])
        if not descriptor.startswith("applications/") or not descriptor.endswith("/apps.toml"):
            raise ValueError("Invalid application descriptor")
        prefix = descriptor[:-len("apps.toml")]
        owned = {f["path"][len(prefix):]: f["size"] for f in files if f["path"].startswith(prefix)}
        if not owned or len(owned) > 4096 or sum(owned.values()) > MAX_BYTES or "apps.toml" not in owned or (app["runtime"].get("main") or "app.lua") not in owned:
            raise ValueError("Incomplete or oversized Kapsule")
        package_path = packages[app["id"]]
        if not isinstance(package_path, str) or not PACKAGE_RE.fullmatch(package_path):
            raise ValueError("Invalid package filename")
        expected.add(package_path)
        package = json.loads((root / package_path).read_text(encoding="utf-8"))
        if package.get("id") != app["id"] or package.get("commit") != catalog["commit"]:
            raise ValueError("Package identity mismatch")
        seen = set()
        for item in package["files"]:
            name = safe_path(item["path"])
            if name not in owned or name in seen or not isinstance(item["data"], str) or len(item["data"]) != 4 * ((owned[name] + 2) // 3):
                raise ValueError("Invalid package member")
            data = base64.b64decode(item["data"], validate=True)
            if len(data) != owned[name]:
                raise ValueError("Package member size mismatch")
            seen.add(name)
        if seen != set(owned):
            raise ValueError("Incomplete package")
    actual = set()
    for file in root.rglob("*"):
        if file.is_symlink():
            raise ValueError("Symlinks are forbidden in the public distribution")
        if file.is_file():
            actual.add(file.relative_to(root).as_posix())
    if actual != expected:
        raise ValueError("Unexpected or missing files in the packaged store")
    return catalog


def public_files(root):
    root = Path(root)
    if root.is_symlink():
        raise ValueError("Distribution directory must not be a symlink")
    files = {}
    for file in root.rglob("*"):
        if file.is_symlink():
            raise ValueError("Symlinks are forbidden in the public distribution")
        if not file.is_file():
            continue
        name = file.relative_to(root).as_posix()
        if name == MANIFEST:
            continue
        if name not in STATIC_FILES and name != "store/catalog.json" and not re.fullmatch(r"store/packages/[0-9a-f]{64}\.json", name):
            raise ValueError(f"Unexpected public file: {name}")
        data = file.read_bytes()
        files[name] = {"size": len(data), "sha256": hashlib.sha256(data).hexdigest()}
    if not STATIC_FILES.issubset(files) or not all(files[name]["size"] for name in STATIC_FILES):
        raise ValueError("Incomplete static distribution")
    if (root / "kapsule.wasm").read_bytes()[:8] != b"\0asm\1\0\0\0":
        raise ValueError("Invalid WebAssembly header")
    validate_store(root / "store")
    return files


def seal(root):
    root = Path(root)
    manifest = {"schema": 1, "files": public_files(root)}
    (root / MANIFEST).write_text(json.dumps(manifest, sort_keys=True, indent=2), encoding="utf-8")


def verify(root):
    root = Path(root)
    manifest = json.loads((root / MANIFEST).read_text(encoding="utf-8"))
    if manifest.get("schema") != 1 or manifest.get("files") != public_files(root):
        raise ValueError("Distribution changed after build; rebuild before publication")


if __name__ == "__main__":
    import sys
    try:
        verify(Path(sys.argv[1]))
    except (ValueError, KeyError, TypeError, OSError) as error:
        raise SystemExit(f"Publication refused: {error}")
