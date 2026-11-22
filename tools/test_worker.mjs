import assert from "node:assert/strict";
import { Worker } from "node:worker_threads";
import { BrowserEngine } from "../web/engine-client.js";
const workerURL = process.env.KAPSULE_WEB_DIR
  ? new URL("file://" + process.env.KAPSULE_WEB_DIR + "/engine-worker.js").href
  : new URL("../dist/web/engine-worker.js", import.meta.url).href;
function createWorker() {
  const code = `import {parentPort} from 'node:worker_threads';
  globalThis.postMessage=(m,t)=>parentPort.postMessage(m,t);
  const queued=[];let ready=false;
  parentPort.on('message',data=>{if(ready)globalThis.onmessage({data});else queued.push(data)});
  await import(${JSON.stringify(workerURL)});ready=true;for(const data of queued)globalThis.onmessage({data});`;
  const worker = new Worker(
    new URL("data:text/javascript," + encodeURIComponent(code)),
    { type: "module" },
  );
  return {
    postMessage: (...args) => worker.postMessage(...args),
    terminate: () => worker.terminate(),
    addEventListener: (kind, fn) =>
      worker.on(kind, kind === "message" ? (data) => fn({ data }) : fn),
  };
}
function files(script) {
  return [
    {
      path: "apps.toml",
      data: new TextEncoder().encode(
        '[application]\nid="probe"\n[runtime]\nmain="app.lua"\n',
      ),
    },
    { path: "app.lua", data: new TextEncoder().encode(script) },
  ];
}
let dialogs = 0;
const engine = new BrowserEngine({
  createWorker,
  timeout: 1000,
  onEvent(message) {
    if (message.type !== "dialog") return;
    dialogs++;
    const state = new Int32Array(message.mailbox, 0, 2),
      bytes = new TextEncoder().encode(JSON.stringify("answer"));
    new Uint8Array(message.mailbox, 8, bytes.length).set(bytes);
    Atomics.store(state, 1, bytes.length);
    Atomics.store(state, 0, 1);
    Atomics.notify(state, 0);
  },
});
try {
  let snapshot = await engine.load(
    files(
      "local d=require('kkc-dialog');assert(d.input('test','')=='answer');return {update=function() end}",
    ),
    [],
    40,
    10,
    "probe",
  );
  assert.equal(dialogs, 1);
  assert.equal(snapshot.cells.length, 400);
  await assert.rejects(
    engine.load(files("while true do end"), [], 40, 10, "probe"),
    /ne répond plus/,
  );
  snapshot = await engine.load(
    files("return {update=function() end}"),
    [],
    40,
    10,
    "probe",
  );
  assert.equal(snapshot.running, true);
  await engine.load(
    files("return {update=function() while true do end end}"),
    [],
    40,
    10,
    "probe",
  );
  await assert.rejects(engine.call({ op: "tick", dt: 0.03 }), /ne répond plus/);
  await engine.load(files("return {}"), [], 40, 10, "probe");
  const pending = engine.load(files("while true do end"), [], 40, 10, "probe");
  await new Promise((r) => setTimeout(r, 100));
  const cancelled = assert.rejects(
    pending,
    (error) => error.name === "AbortError",
  );
  await engine.stop();
  await cancelled;
  snapshot = await engine.load(files("return {}"), [], 40, 10, "probe");
  assert.equal(snapshot.running, true);
  console.log(
    "Worker: synchronous dialogs, hung startup/update termination, cancellation and recovery passed",
  );
} finally {
  engine.fail(new Error("Test complete"));
}
