#!/usr/bin/env python3
"""Package store resources as JSON data, never as Apache-executable source files."""
import base64
import concurrent.futures
import hashlib
import json
from pathlib import Path
import shutil
import tempfile
import urllib.parse
import urllib.request
from web_distribution import MAX_BYTES, safe_path, validate_store

REPO = "redbug26/kkc-store"
API = f"https://api.github.com/repos/{REPO}"
RAW = f"https://raw.githubusercontent.com/{REPO}"


def download(url, limit=MAX_BYTES):
    request = urllib.request.Request(url, headers={"User-Agent": "kapsule-rust-store-builder"})
    with urllib.request.urlopen(request, timeout=45) as response:
        data = response.read(limit + 1)
        if len(data) > limit:
            raise ValueError("Store response exceeds size limit")
        return data


def sync_store(destination, cache=None):
    destination = Path(destination)
    commit = json.loads(download(f"{API}/commits/main", 8 * 1024 * 1024))["sha"]
    if not isinstance(commit, str) or len(commit) != 40 or any(c not in "0123456789abcdef" for c in commit):
        raise ValueError("Invalid commit")
    for source in [destination, Path(cache) if cache else destination]:
        try:
            cached = validate_store(source)
            if cached["commit"] == commit:
                if source != destination:
                    shutil.copytree(source, destination)
                print(f"Store current: {len(cached['applications'])} Kapsules, commit {commit[:12]}")
                return
        except (ValueError, KeyError, TypeError, OSError):
            pass
    index = json.loads(download(f"{RAW}/{commit}/dist/store-index.json", 8 * 1024 * 1024))
    tree = json.loads(download(f"{API}/git/trees/{commit}?recursive=1", 8 * 1024 * 1024))
    if tree.get("truncated"):
        raise ValueError("Truncated repository tree")
    apps = []
    for item in index["applications"]:
        if not isinstance(item.get("runtime"), dict):
            continue
        descriptor = safe_path(item["descriptor"])
        if not descriptor.startswith("applications/") or not descriptor.endswith("/apps.toml"):
            raise ValueError("Invalid descriptor")
        apps.append({k: item.get(k, "") for k in ("id", "name", "version", "description", "descriptor", "runtime")})
    if not apps or any(not isinstance(a["id"], str) or not a["id"] for a in apps) or len({a["id"] for a in apps}) != len(apps):
        raise ValueError("Empty or ambiguous store")
    prefixes = [a["descriptor"][:-len("apps.toml")] for a in apps]
    files = []
    for item in tree["tree"]:
        if item["type"] != "blob" or not any(item["path"].startswith(prefix) for prefix in prefixes):
            continue
        safe_path(item["path"])
        if item["mode"] not in ("100644", "100755") or type(item.get("size")) is not int or item["size"] < 0:
            raise ValueError("Unsupported store file")
        files.append({"path": item["path"], "size": item["size"]})
    if sum(f["size"] for f in files) > 256 * 1024 * 1024:
        raise ValueError("Store snapshot exceeds 256 MiB")
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="kapsule-store-", dir=destination.parent) as temporary:
        staging = Path(temporary)
        (staging / "packages").mkdir()
        packages = {}
        for app, prefix in zip(apps, prefixes):
            owned = [f for f in files if f["path"].startswith(prefix)]
            if len(owned) > 4096 or sum(f["size"] for f in owned) > MAX_BYTES:
                raise ValueError("Kapsule exceeds download limits")
            def member(item):
                data = download(f"{RAW}/{commit}/{urllib.parse.quote(item['path'], safe='/')}")
                if len(data) != item["size"]:
                    raise ValueError("Store file size mismatch")
                return {"path": item["path"][len(prefix):], "data": base64.b64encode(data).decode("ascii")}
            with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
                members = list(pool.map(member, owned))
            name = f"packages/{hashlib.sha256(app['descriptor'].encode()).hexdigest()}.json"
            packages[app["id"]] = name
            (staging / name).write_text(json.dumps({"id": app["id"], "commit": commit, "files": members}), encoding="utf-8")
        (staging / "catalog.json").write_text(json.dumps({"format": 2, "commit": commit, "applications": apps, "files": files, "packages": packages}, ensure_ascii=False, indent=2), encoding="utf-8")
        validate_store(staging)
        if destination.exists():
            shutil.rmtree(destination)
        shutil.copytree(staging, destination)
    print(f"Store: {len(apps)} Kapsules, {len(files)} resources packed as JSON, commit {commit[:12]}")


if __name__ == "__main__":
    import sys
    sync_store(Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parents[1] / "dist/web/store")
