import createKapsule from "./kapsule.js";
import { validateFiles } from "./files.js";
let runtime,
  loaded = false,
  playing = false,
  dialogCount = 0,
  resetKeys = false;
const send = (message) => globalThis.postMessage(message);
function invoke(request) {
  const response = JSON.parse(
    runtime.ccall(
      "kapsule_call",
      "string",
      ["string"],
      [JSON.stringify(request)],
    ),
  );
  if (resetKeys && !response.error) {
    resetKeys = false;
    runtime.ccall(
      "kapsule_call",
      "string",
      ["string"],
      [JSON.stringify({ op: "focus", active: false })],
    );
  }
  if (response.error) throw new Error(response.error);
  return response;
}
globalThis.kapsuleAudio = {
  beep() {
    send({ type: "audio", action: "beep" });
  },
  play(path) {
    try {
      const bytes = runtime.FS.readFile(path);
      playing = false;
      globalThis.postMessage({ type: "audio", action: "play", bytes }, [
        bytes.buffer,
      ]);
      return true;
    } catch {
      return false;
    }
  },
  stop() {
    playing = false;
    send({ type: "audio", action: "stop" });
  },
  playing() {
    return playing;
  },
};
globalThis.kapsuleDialog = (kind, text, fallback) => {
  if (typeof SharedArrayBuffer === "undefined")
    return {
      __kapsule_error:
        "Browser dialogs require cross-origin isolation (COOP/COEP headers).",
    };
  if (++dialogCount > 32)
    return { __kapsule_error: "Too many dialogs in one callback." };
  const mailbox = new SharedArrayBuffer(65544),
    state = new Int32Array(mailbox, 0, 2);
  send({ type: "dialog", kind, text, fallback, mailbox });
  if (Atomics.wait(state, 0, 0, 120000) === "timed-out")
    return { __kapsule_error: "Dialog timed out." };
  const length = Atomics.load(state, 1);
  resetKeys = true;
  if (length < 0 || length > 65536)
    return { __kapsule_error: "Dialog response is too large." };
  return JSON.parse(
    new TextDecoder().decode(new Uint8Array(mailbox, 8, length)),
  );
};
globalThis.onmessage = (event) => {
  const message = event.data;
  if (message.type === "audio-status") {
    playing = !!message.playing;
    return;
  }
  const { id, type } = message;
  try {
    dialogCount = 0;
    let result;
    if (type === "load") {
      if (loaded)
        throw new Error("A fresh worker is required for each Kapsule.");
      const { files, args, width, height, expectedId } = message;
      validateFiles(files);
      loaded = true;
      runtime.FS.mkdirTree("/apps/session");
      for (const file of files) {
        const path = `/apps/session/${file.path}`;
        runtime.FS.mkdirTree(path.slice(0, path.lastIndexOf("/")));
        runtime.FS.writeFile(path, file.data);
      }
      result = invoke({
        op: "start",
        path: "/apps/session/apps.toml",
        args,
        width,
        height,
        expected_id: expectedId,
      });
    } else if (type === "call") {
      result = invoke(message.request);
    } else throw new Error("Unknown engine request.");
    send({ type: "response", id, result });
  } catch (error) {
    send({ type: "response", id, error: error.message || String(error) });
  }
};
try {
  runtime = await createKapsule({ print: () => {}, printErr: () => {} });
  send({ type: "ready" });
} catch (error) {
  send({ type: "fatal", error: error.message || String(error) });
}
