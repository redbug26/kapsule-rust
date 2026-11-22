#!/usr/bin/env python3
"""Exercise the real terminal host through a disposable Unix PTY."""
import errno
import fcntl
import json
import os
from pathlib import Path
import pty
import re
import select
import signal
import struct
import sys
import subprocess
import tempfile
import termios
import time

ROOT = Path(__file__).resolve().parents[1]
metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--no-deps", "--format-version", "1"], cwd=ROOT))
BINARY = Path(metadata["target_directory"]) / "debug/kapsule"
subprocess.run(["cargo", "build", "--locked"], cwd=ROOT, check=True)


def exercise(script, actions, expected_code=0, absent=None, enhanced=False):
    with tempfile.TemporaryDirectory(prefix="kapsule-pty-") as tmp:
        app = Path(tmp)
        (app / "apps.toml").write_text("[application]\nid='pty'\n[runtime]\nwidth=40\nheight=16\n")
        (app / "app.lua").write_text(script)
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))

        def child_terminal():
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)

        restored = app / "restored.json"
        wrapper = """import json, subprocess, sys, termios
original = termios.tcgetattr(0)
result = subprocess.run(sys.argv[1:3])
with open(sys.argv[3], 'w') as output:
    json.dump(termios.tcgetattr(0) == original, output)
sys.exit(result.returncode)
"""
        process = subprocess.Popen([sys.executable, "-c", wrapper, str(BINARY), str(app), str(restored)], stdin=slave, stdout=slave, stderr=slave,
                                   preexec_fn=child_terminal, env={**os.environ, "TERM": "xterm-256color"})
        captured = bytearray()
        replied_to_probe = False
        warning_acknowledged = False

        def screen_contains(marker):
            # Ratatui updates changed cells only; reconstruct the character grid
            # so markers remain readable when equal characters are skipped.
            grid = [[" "] * 160 for _ in range(60)]
            x = y = 0
            text = captured.decode(errors="replace")
            tokens = re.split(r"(\x1b\[[0-?]*[ -/]*[@-~])", text)
            for token in tokens:
                if token.startswith("\x1b["):
                    if token == "\x1b[2J":
                        grid = [[" "] * 160 for _ in range(60)]
                    if token[-1] in "Hf":
                        values = token[2:-1].split(";")
                        y = int(values[0] or 1) - 1
                        x = int(values[1] or 1) - 1 if len(values) > 1 else 0
                    continue
                for char in token:
                    if char == "\r": x = 0
                    elif char == "\n": y += 1
                    elif char >= " " and char != "\x7f":
                        if 0 <= y < 60 and 0 <= x < 160: grid[y][x] = char
                        x += 1
            return marker.decode() in "\n".join("".join(row) for row in grid)

        def receive(until=None, timeout=5):
            nonlocal replied_to_probe, warning_acknowledged
            deadline = time.monotonic() + timeout
            while time.monotonic() < deadline:
                if select.select([master], [], [], 0.05)[0]:
                    try:
                        data = os.read(master, 65536)
                    except OSError as error:
                        if error.errno == errno.EIO:
                            break
                        raise
                    if not data:
                        break
                    captured.extend(data)
                    if not replied_to_probe and b"\x1b[?u" in captured:
                        flags = b"\x1b[?0u" if enhanced else b""
                        os.write(master, flags + b"\x1b[?1;2c")
                        replied_to_probe = True
                    if not warning_acknowledged and b"Ce terminal ne transmet pas" in captured:
                        os.write(master, b"\r")
                        warning_acknowledged = True
                if until and (until in captured or screen_contains(until)):
                    return
                if process.poll() is not None:
                    break
            if until:
                assert until in captured or screen_contains(until), (until, captured[-1000:].decode(errors="replace"))

        try:
            for data, marker in actions:
                if data == "resize":
                    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 100, 0, 0))
                    os.kill(process.pid, signal.SIGWINCH)
                elif data:
                    os.write(master, data)
                receive(marker)
            if absent:
                assert not screen_contains(absent), "Dialog contents remained on the parent screen"
            process.wait(timeout=5)
            receive()
            assert process.returncode == expected_code, captured.decode(errors="replace")
            assert warning_acknowledged == (not enhanced), "Warning must match negotiated key-up support"
            assert json.loads(restored.read_text()), "Terminal attributes were not restored"
            assert b"\x1b[?1049l" in captured, "Alternate screen was not restored"
        finally:
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
            os.close(master)
            os.close(slave)


exercise("""
local gfx=require('kkc-graphics')
local kkc=require('kkc')
local marker='READY'
return {
resize=function(w,h) if w==38 then marker='WINDOWED' elseif w==98 then marker='RESIZED' end end,
mousepressed=function(b,x,y) assert(b=='left' and x==3 and y==4); marker='MOUSE_OK' end,
keypressed=function(k) if k=='char:x' then marker='KEY_OK' elseif k=='esc' then kkc.quit() end end,
draw=function() gfx.clear(); gfx.text(1,1,marker) end
}
""", [(None, b"READY"), (b"\x1b[15~", b"WINDOWED"), (b"x", b"KEY_OK"),
       (b"\x1b[<0;3;4M", b"MOUSE_OK"), (b"\x1b[15~", None), ("resize", b"RESIZED"), (b"\x1b", None)])
exercise("""
local gfx=require('kkc-graphics')
local status='BEFORE_DIALOG'
return {
keypressed=function(k)
    if k=='char:d' then require('kkc-dialog').message('DIALOG_TEMP'); status='AFTER_DIALOG'
    elseif k=='esc' then require('kkc').quit() end
end,
draw=function() gfx.clear('.'); gfx.text(1,1,status) end
}
""", [(None, b"BEFORE_DIALOG"), (b"d", b"DIALOG_TEMP"), (b"\r", b"AFTER_DIALOG"), (b"\x1b", None)], absent=b"DIALOG_TEMP")
exercise("return {draw=function() error('PTY_FAILURE') end}", [(None, b"PTY_FAILURE")], 1)
exercise("return {init=function() require('kkc-dialog').message('DIALOG_READY') end}",
         [(None, b"DIALOG_READY"), (b"\x03", b"Dialog interrupted")], 1)
exercise("""
local gfx=require('kkc-graphics')
local key=require('kkc-key')
local kkc=require('kkc')
local marker='ENHANCED_READY'
local pressed_at=nil
local downs, presses=0,0
return {
init=function() assert(key.HAS_RELEASE_EVENTS) end,
keydown=function(k)
    if k=='right' then downs=downs+1; pressed_at=kkc.time(); assert(key.is_down('right')) end
end,
keypressed=function(k) if k=='right' then presses=presses+1 end end,
update=function()
    if pressed_at and kkc.time()-pressed_at > 0.55 then
        assert(key.is_down('right'), 'Held arrow expired before key up')
        assert(downs==1 and presses==1, 'The host must not synthesize keyboard repeats')
        marker='HELD_UNTIL_RELEASE'
    end
end,
keyup=function(k)
    if k=='right' then assert(not key.is_down('right')); pressed_at=nil; marker='RELEASED' end
end,
draw=function() gfx.clear(); gfx.text(1,1,marker) end
}
""", [(None, b"ENHANCED_READY"), (b"\x1b[1;1:1C", b"HELD_UNTIL_RELEASE"),
       (b"\x1b[1;1:3C", b"RELEASED"), (b"\x03", None)], enhanced=True)
print("PTY: legacy/enhanced keyboard, key-up warning, F5, mouse, resize, dialogs and terminal restoration passed")
