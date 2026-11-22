import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
const webRoot = process.env.KAPSULE_WEB_DIR;
import { fileURLToPath, pathToFileURL } from "node:url";
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const { default: createKapsule } = await import(
  pathToFileURL(path.join(webRoot || path.join(root, "dist/web"), "kapsule.js"))
);
globalThis.kapsuleAudio = {
  beep() {},
  play() {
    return false;
  },
  stop() {},
  playing() {
    return false;
  },
};
globalThis.kapsuleDialog = (kind, text, fallback) =>
  kind === "message" ? null : kind === "confirm" ? fallback : "answer";
const runtime = await createKapsule();
function call(request) {
  const response = JSON.parse(
    runtime.ccall(
      "kapsule_call",
      "string",
      ["string"],
      [JSON.stringify(request)],
    ),
  );
  assert.equal(response.error, undefined, response.error);
  return response;
}
const store = JSON.parse(
  fs.readFileSync(
    path.join(webRoot || path.join(root, "dist/web"), "store/catalog.json"),
    "utf8",
  ),
);
let compatible = 0,
  unsupported = 0;
for (const [index, app] of store.applications.entries()) {
  const directory = `/tests/store-${index}`;
  runtime.FS.mkdirTree(directory);
  const payload = JSON.parse(
    fs.readFileSync(
      path.join(
        webRoot || path.join(root, "dist/web"),
        "store",
        store.packages[app.id],
      ),
      "utf8",
    ),
  );
  for (const file of payload.files) {
    const target = directory + "/" + file.path;
    runtime.FS.mkdirTree(path.posix.dirname(target));
    runtime.FS.writeFile(target, Buffer.from(file.data, "base64"));
  }
  let result = JSON.parse(
    runtime.ccall(
      "kapsule_call",
      "string",
      ["string"],
      [
        JSON.stringify({
          op: "start",
          path: `${directory}/apps.toml`,
          expected_id: app.id,
          args: [],
          width: 94,
          height: 30,
        }),
      ],
    ),
  );
  if (result.error?.includes("kkc-shell.run is unavailable")) {
    unsupported++;
    call({ op: "stop" });
    continue;
  }
  assert.equal(result.error, undefined, `${app.id}: ${result.error}`);
  assert.equal(result.cells.length, 94 * 30);
  for (let i = 0; i < 5; i++) result = call({ op: "tick", dt: 1 / 30 });
  assert.equal(result.fg.length, result.cells.length);
  call({ op: "resize", width: 78, height: 22 });
  call({ op: "focus", active: false });
  call({ op: "stop" });
  compatible++;
}
// Exercise the real browser C ABI, virtual FS, module API and keyboard lifecycle.
runtime.FS.mkdirTree("/probe");
runtime.FS.writeFile(
  "/probe/apps.toml",
  '[application]\nid="probe"\n[runtime]\nmain="app.lua"\n',
);
runtime.FS.writeFile(
  "/probe/app.lua",
  `
local key=require('kkc-key')
local gfx=require('kkc-graphics')
local fs=require('kkc-fs')
local dialog=require('kkc-dialog')
assert(key.HAS_RELEASE_EVENTS)
assert(require('kkc').args()[1]=='--literal')
assert(require('kkc').get_cwd()=='/probe')
local ok,err=pcall(require('kkc-shell').run,'git')
assert(not ok and tostring(err):find('unavailable'))
fs.write_text('saved.txt','virtual storage')
assert(fs.read_text('saved.txt')=='virtual storage')
assert(dialog.input('prompt','default')=='answer')
local selection=dialog.select_with_checks('empty',{},1,{{label='Saved',checked=true}})
assert(selection.index==nil and selection.checks[1])
local spec=dialog.confirm_box({buttons={items={{}}},callback=function() return 'yes' end})
assert(spec.buttons.items[1].callback=='yes')
local downs,presses=0,0
return {
keydown=function(k) assert(k=='right'); downs=downs+1 end,
keypressed=function(k) presses=presses+1 end,
keyup=function(k) assert(not key.is_down(k)) end,
focuslost=function() assert(not key.is_down('right')) end,
update=function() gfx.clear(); gfx.text(1,1,key.is_down('right') and 'HELD' or 'UP'); gfx.text(1,2,downs..':'..presses) end
}
`,
);
const mismatch = JSON.parse(
  runtime.ccall(
    "kapsule_call",
    "string",
    ["string"],
    [
      JSON.stringify({
        op: "start",
        path: "/probe/apps.toml",
        expected_id: "wrong-id",
        args: [],
        width: 40,
        height: 10,
      }),
    ],
  ),
);
assert.match(mismatch.error, /ID does not match/);
call({
  op: "start",
  path: "/probe/apps.toml",
  args: ["--literal"],
  width: 40,
  height: 10,
});
call({ op: "key", name: "right", kind: 0 });
await new Promise((resolve) => setTimeout(resolve, 550));
let result = call({ op: "tick", dt: 0.55 });
assert.equal(String.fromCodePoint(...result.cells.slice(0, 4)), "HELD");
assert.equal(String.fromCodePoint(...result.cells.slice(40, 43)), "1:1");
call({ op: "key", name: "right", kind: 2 });
result = call({ op: "tick", dt: 0.03 });
assert.equal(String.fromCodePoint(...result.cells.slice(0, 2)), "UP");
call({ op: "key", name: "right", kind: 0 });
call({ op: "focus", active: false });
result = call({ op: "tick", dt: 0.03 });
assert.equal(String.fromCodePoint(...result.cells.slice(0, 2)), "UP");
const error = JSON.parse(
  runtime.ccall(
    "kapsule_call",
    "string",
    ["string"],
    [JSON.stringify({ op: "resize", width: 65535, height: 65535 })],
  ),
);
assert.match(error.error, /Canvas size/);
call({ op: "stop" });
console.log(
  `WASM: ${compatible} store Kapsules, ${unsupported} explicit process rejection(s), manifest identity, frames, files, keys and focus passed`,
);
