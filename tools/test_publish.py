#!/usr/bin/env python3
"""Exercise rebuilding, validation, rsync and cleanup without network access."""
import json
import os
from pathlib import Path
import shutil
import signal
import time
import subprocess
import tempfile
from web_distribution import verify

ROOT = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="kapsule-publish-test-") as temporary:
    root = Path(temporary)
    shutil.copy2(ROOT / "publish.sh", root / "publish.sh")
    tools = root / "tools"
    tools.mkdir()
    shutil.copy2(ROOT / "tools/web_distribution.py", tools)
    fixture = root / "fixture"
    shutil.copytree(Path(os.environ.get("KAPSULE_WEB_DIR", ROOT / "dist/web")), fixture)
    verify(fixture)
    (tools / "build_wasm.py").write_text('''import argparse,json,os,shutil
from pathlib import Path
if os.environ.get('KAPSULE_TEST_REQUIRE_SDK'): assert os.environ.get('KAPSULE_TEST_SDK_ACTIVE')=='yes'
p=argparse.ArgumentParser();p.add_argument('--out-dir');p.add_argument('--target-dir');a=p.parse_args()
Path(os.environ['BUILD_TEST_LOG']).write_text(json.dumps(vars(a)))
Path(a.target_dir).mkdir();Path(a.target_dir,'temporary-object.o').write_text('generated')
if os.environ.get('BUILD_TEST_WAIT'):
 import time;time.sleep(30)
if os.environ.get('BUILD_TEST_FAIL'): raise SystemExit(9)
shutil.copytree(os.environ['BUILD_TEST_FIXTURE'],a.out_dir)
''')
    binaries = root / "bin"
    binaries.mkdir()
    sdk = root / "SDK with spaces"
    sdk.mkdir()
    (sdk / "emsdk_env.sh").write_text('export KAPSULE_TEST_SDK_ACTIVE=yes\nexport PATH="'+str(binaries)+':$PATH"\n')
    emcc = binaries / "emcc"
    emcc.write_text('#!/bin/sh\nexit 0\n')
    emcc.chmod(0o755)
    rsync = binaries / "rsync"
    rsync.write_text('''#!/usr/bin/env python3
import json,os,sys
from pathlib import Path
assert Path(sys.argv[-2], 'build-manifest.json').is_file()
Path(os.environ['PUBLISH_TEST_LOG']).write_text(json.dumps(sys.argv[1:]))
sys.exit(int(os.environ.get('RSYNC_TEST_FAIL','0')))
''')
    rsync.chmod(0o755)
    log, build_log = root / "rsync.json", root / "build.json"
    env = {**os.environ, "PATH": str(binaries)+os.pathsep+os.environ["PATH"], "PUBLISH_TEST_LOG": str(log), "BUILD_TEST_LOG": str(build_log), "BUILD_TEST_FIXTURE": str(fixture)}

    def run(options=(), extra=None):
        log.unlink(missing_ok=True)
        build_log.unlink(missing_ok=True)
        result = subprocess.run(["bash", str(root / "publish.sh"), *options], cwd="/tmp", env={**env, **(extra or {})}, capture_output=True, text=True)
        if build_log.exists():
            built = json.loads(build_log.read_text())
            assert not Path(built['out_dir']).parent.exists(), 'Publication staging leaked'
            assert not Path(built['target_dir']).exists(), 'Compilation output leaked'
        return result

    for options in [("--dry-run",), (), ("--build",)]:
        result = run(options)
        assert result.returncode == 0, result.stderr
        assert build_log.exists(), 'Every publication must build and refresh'
        args = json.loads(log.read_text())
        assert args[-1] == "kardkiss@alwaysdata:/home/kardkiss/kapsule.kyuran.be/"
        assert args[-2].endswith('/web/')
        assert ('--dry-run' in args) == ('--dry-run' in options)
        assert '--delete' in args and args[args.index('-e')+1] == 'ssh'
    activated = run(("--dry-run",), {"KAPSULE_EMSDK": str(sdk), "KAPSULE_TEST_REQUIRE_SDK":"1"})
    assert activated.returncode == 0 and 'Activating Emscripten SDK:' in activated.stdout, activated.stderr
    assert run(("--unknown",)).returncode == 2 and not build_log.exists()
    assert run(extra={"BUILD_TEST_FAIL":"1"}).returncode == 9 and not log.exists()
    assert run(extra={"RSYNC_TEST_FAIL":"23"}).returncode == 23 and log.exists()
    build_log.unlink()
    process = subprocess.Popen(["bash", str(root / "publish.sh")], env={**env,"BUILD_TEST_WAIT":"1"}, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    try:
        deadline=time.monotonic()+5
        while not build_log.exists() and time.monotonic()<deadline:
            time.sleep(0.02)
        assert build_log.exists(), 'Interrupted build never started'
        built=json.loads(build_log.read_text())
        os.killpg(process.pid,signal.SIGTERM)
        assert process.wait(timeout=5)==143
        assert not Path(built['out_dir']).parent.exists(), 'Interrupted publication staging leaked'
    finally:
        if process.poll() is None:
            os.killpg(process.pid,signal.SIGKILL);process.wait()
    (fixture / "unexpected.env").write_text('should never upload')
    result = run()
    assert result.returncode != 0 and not log.exists() and 'Unexpected public file' in result.stderr
    (fixture / "unexpected.env").unlink()
    (fixture / "app.js").write_text('tampered')
    assert run().returncode != 0 and not log.exists()
print("Publish: unconditional rebuild/refresh, validation, destination, dry run, build/rsync failures, interruption and temporary cleanup passed; no connection made")
