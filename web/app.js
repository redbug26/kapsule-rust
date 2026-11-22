import { BrowserEngine } from "./engine-client.js";
import { validateFiles } from "./files.js";
import { loadCatalog, downloadApplication } from "./store.js";
const $ = (id) => document.getElementById(id);
const canvas = $("screen"),
  context = canvas.getContext("2d");
const FONT = "16px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace";
context.font = FONT;
const CELL_W = Math.ceil(context.measureText("M").width),
  CELL_H = 20;
let module,
  running = false,
  frame,
  fps = 30,
  lastTick = 0,
  zoomed = false,
  starting = false;
const held = new Map();
let storeCatalog = null,
  loadController = null;
let audioContext,
  audio,
  audioUrl,
  inDialog = false;
function status(text, error = false) {
  $("error-details").hidden = true;
  $("status").textContent = text;
  $("status").classList.toggle("error", error);
}
function unlockAudio() {
  audioContext ??= new (window.AudioContext || window.webkitAudioContext)();
  audioContext.resume().catch(() => {});
}
globalThis.kapsuleAudio = {
  beep() {
    if (!audioContext) return;
    const osc = audioContext.createOscillator(),
      gain = audioContext.createGain();
    gain.gain.value = 0.06;
    osc.frequency.value = 660;
    osc.connect(gain).connect(audioContext.destination);
    osc.start();
    osc.stop(audioContext.currentTime + 0.06);
  },
  play(bytes) {
    try {
      this.stop();
      audioUrl = URL.createObjectURL(new Blob([bytes]));
      audio = new Audio(audioUrl);
      audio.addEventListener("playing", () => module?.audioStatus(true));
      audio.addEventListener("ended", () => module?.audioStatus(false));
      audio.addEventListener("pause", () => module?.audioStatus(false));
      audio.play().catch((error) => status(`Audio : ${error.message}`, true));
      return true;
    } catch {
      return false;
    }
  },
  stop() {
    audio?.pause();
    audio = null;
    if (audioUrl) URL.revokeObjectURL(audioUrl);
    audioUrl = null;
  },
  playing() {
    return !!audio && !audio.paused && !audio.ended;
  },
};
function engineEvent(message) {
  if (message.type === "audio") {
    if (message.action === "beep") globalThis.kapsuleAudio.beep();
    if (message.action === "play") globalThis.kapsuleAudio.play(message.bytes);
    if (message.action === "stop") globalThis.kapsuleAudio.stop();
    return;
  }
  if (message.type !== "dialog") return;
  inDialog = true;
  held.clear();
  let answer;
  try {
    if (message.kind === "message") {
      window.alert(message.text);
      answer = null;
    } else if (message.kind === "confirm") {
      const value = window.prompt(
        `${message.text}\nY / N`,
        message.fallback ? "Y" : "N",
      );
      answer =
        value === null || !value.trim()
          ? message.fallback
          : value.trim().toLowerCase().startsWith("y");
    } else answer = window.prompt(message.text, message.fallback ?? "");
  } finally {
    const state = new Int32Array(message.mailbox, 0, 2);
    const bytes = new TextEncoder().encode(JSON.stringify(answer ?? null));
    if (bytes.length <= 65536) {
      new Uint8Array(message.mailbox, 8, bytes.length).set(bytes);
      Atomics.store(state, 1, bytes.length);
    } else Atomics.store(state, 1, -1);
    Atomics.store(state, 0, 1);
    Atomics.notify(state, 0);
    inDialog = false;
    lastTick = 0;
  }
}
async function call(request) {
  return module.call(request);
}
function showError(error) {
  const text = error.message || String(error);
  const friendly = text.includes("kkc-shell.run is unavailable")
    ? "Cette Kapsule nécessite un programme externe. Utilisez le lecteur natif."
    : text.includes("memory") || text.includes("Memory")
      ? "La Kapsule a dépassé les ressources disponibles. Vous pouvez en lancer une autre."
      : text;
  status(friendly, true);
  $("error-details").hidden = friendly === text;
  $("error-text").textContent = text;
}
async function guard(action) {
  try {
    return await action();
  } catch (error) {
    if (error.name === "AbortError") return;
    running = false;
    $("stop").disabled = true;
    $("zoom").disabled = true;
    globalThis.kapsuleAudio.stop();
    showError(error);
  }
}
function rgb(value) {
  return `#${value.toString(16).padStart(6, "0")}`;
}
function paint(snapshot) {
  if (!snapshot.cells) return;
  frame = snapshot;
  fps = snapshot.fps;
  running = snapshot.running;
  $("title").textContent = snapshot.name;
  $("dimensions").textContent = `${snapshot.width} × ${snapshot.height}`;
  $("stop").disabled = !running;
  $("zoom").disabled = !running;
  $("welcome").hidden = true;
  const w = snapshot.width * CELL_W,
    h = snapshot.height * CELL_H,
    scale = window.devicePixelRatio || 1;
  if (
    canvas.width !== Math.round(w * scale) ||
    canvas.height !== Math.round(h * scale)
  ) {
    canvas.width = Math.round(w * scale);
    canvas.height = Math.round(h * scale);
  }
  const fit = Math.min(
    1,
    ($("viewport").clientWidth - 24) / w,
    ($("viewport").clientHeight - 12) / h,
  );
  canvas.style.width = `${w * fit}px`;
  canvas.style.height = `${h * fit}px`;
  context.setTransform(scale, 0, 0, scale, 0, 0);
  context.font = FONT;
  context.textBaseline = "top";
  for (let row = 0; row < snapshot.height; row++) {
    const base = row * snapshot.width;
    for (let col = 0; col < snapshot.width; ) {
      const color = snapshot.bg[base + col];
      let end = col + 1;
      while (end < snapshot.width && snapshot.bg[base + end] === color) end++;
      context.fillStyle = rgb(color);
      context.fillRect(
        col * CELL_W,
        row * CELL_H,
        (end - col) * CELL_W,
        CELL_H,
      );
      col = end;
    }
    for (let col = 0; col < snapshot.width; col++) {
      const index = base + col,
        code = snapshot.cells[index];
      if (code <= 32 || (code >= 0x7f && code < 0xa0)) continue;
      context.fillStyle = rgb(snapshot.fg[index]);
      context.fillText(
        String.fromCodePoint(code),
        col * CELL_W,
        row * CELL_H + 1,
        CELL_W,
      );
    }
  }
  if (snapshot.shortcuts.length) {
    let col = 0;
    const y = h - CELL_H;
    context.fillStyle = "#463223";
    context.fillRect(0, y, w, CELL_H);
    for (const item of snapshot.shortcuts) {
      const key = ` ${item.key} `,
        label = ` ${item.label}  `,
        width = [...key].length * CELL_W;
      context.fillStyle = "#d2b48c";
      context.fillRect(col, y, width, CELL_H);
      context.fillStyle = "#000";
      context.fillText(key, col, y + 1);
      col += width;
      context.fillStyle = "#f0dcb4";
      context.fillText(label, col, y + 1);
      col += [...label].length * CELL_W;
    }
  }
  if (!running) {
    status("Kapsule terminée. Vous pouvez en lancer une autre.");
    globalThis.kapsuleAudio.stop();
  }
}
function size() {
  if (!zoomed && frame)
    return { width: defaultSize.width, height: defaultSize.height };
  return {
    width: Math.max(
      20,
      Math.min(240, Math.floor(($("viewport").clientWidth - 24) / CELL_W)),
    ),
    height: Math.max(
      8,
      Math.min(120, Math.floor(($("viewport").clientHeight - 20) / CELL_H)),
    ),
  };
}
let defaultSize = { width: 94, height: 30 };
async function toggleZoom() {
  if (!running) return;
  zoomed = !zoomed;
  paint(await call({ op: "resize", ...size() }));
  canvas.focus();
}
async function tick(now) {
  if (
    running &&
    !document.hidden &&
    (!lastTick || now - lastTick >= 1000 / fps)
  ) {
    const dt = lastTick ? Math.min((now - lastTick) / 1000, 0.1) : 1 / fps;
    lastTick = now;
    await guard(async () => paint(await call({ op: "tick", dt })));
  }
  requestAnimationFrame(tick);
}
async function start(files, expectedId = null) {
  const args = JSON.parse($("args").value);
  if (!Array.isArray(args) || args.some((arg) => typeof arg !== "string"))
    throw new Error("Les arguments doivent être un tableau JSON de chaînes.");
  validateFiles(files);
  running = false;
  held.clear();
  $("stop").disabled = true;
  $("zoom").disabled = true;
  // Rust parses the manifest. These defaults keep the browser canvas predictable.
  defaultSize = { width: 94, height: 30 };
  frame = null;
  zoomed = true;
  lastTick = 0;
  starting = true;
  let snapshot;
  try {
    const dimensions = size();
    snapshot = await module.load(
      files,
      args,
      dimensions.width,
      dimensions.height,
      expectedId,
    );
  } finally {
    starting = false;
  }
  defaultSize = {
    width: snapshot.window_width,
    height: snapshot.window_height,
  };
  paint(snapshot);
  status("Clavier actif : appui et relâchement des touches pris en charge.");
  canvas.focus();
}
function selectedDescription() {
  const id = $("demo").value.replace(/^store:/, "");
  const app = storeCatalog?.applications.find((app) => app.id === id);
  $("store-description").textContent = app
    ? `${app.name}${app.version ? ` · ${app.version}` : ""} — ${app.description}`
    : "Hello Kapsule : un premier exemple. Le store propose toutes les Kapsules Lua publiées.";
}
function populateStore(preferred) {
  const previous = preferred || $("demo").value;
  const group = document.createElement("optgroup");
  group.label = "Store KKC";
  for (const app of storeCatalog.applications)
    group.append(new Option(app.name, `store:${app.id}`));
  const local = document.createElement("optgroup");
  local.label = "Exemple local";
  local.append(new Option("Hello Kapsule", "local:hello"));
  $("demo").replaceChildren(group, local);
  if ([...$("demo").options].some((option) => option.value === previous))
    $("demo").value = previous;
  $("store-source").textContent =
    `${storeCatalog.applications.length} Kapsules · ${storeCatalog.source === "github" ? "GitHub" : "copie embarquée"}`;
  selectedDescription();
}
async function loading(action) {
  if (loadController) return;
  const controller = new AbortController();
  loadController = controller;
  for (const id of ["launch", "demo", "folder", "refresh-store"])
    $(id).disabled = true;
  $("cancel-load").hidden = false;
  try {
    await action(controller.signal);
  } catch (error) {
    if (controller.signal.aborted || error.name === "AbortError")
      status("Chargement annulé.");
    else showError(error);
  } finally {
    loadController = null;
    $("cancel-load").hidden = true;
    for (const id of ["launch", "demo", "folder", "refresh-store"])
      $(id).disabled = !module;
  }
}
async function refreshStore(preferred) {
  await loading(async (signal) => {
    status("Chargement du catalogue du store…");
    storeCatalog = await loadCatalog({ signal });
    populateStore(preferred);
    status(
      storeCatalog.source === "github"
        ? "Store à jour. Choisissez une Kapsule puis lancez-la."
        : "GitHub est momentanément indisponible. Le catalogue embarqué reste disponible.",
    );
  });
}
$("demo").addEventListener("change", selectedDescription);
$("cancel-load").addEventListener("click", () => {
  loadController?.abort();
  if (starting) module.stop();
});
$("refresh-store").addEventListener("click", () => refreshStore());
$("launch").addEventListener("click", () => {
  unlockAudio();
  loading(async (signal) => {
    const choice = $("demo").value;
    if (choice.startsWith("store:")) {
      const id = choice.slice(6);
      const files = await downloadApplication(storeCatalog, id, {
        signal,
        onProgress: (done, total, app) =>
          status(`Chargement de ${app.name} : ${done}/${total} fichiers…`),
      });
      if (signal.aborted) return;
      await start(files, id);
      const url = new URL(location.href);
      url.searchParams.set("kapsule", id);
      history.replaceState(null, "", url);
    } else {
      const response = await fetch("./demos.json", { signal });
      if (!response.ok) throw new Error("Exemple local introuvable.");
      const demos = await response.json(),
        encoder = new TextEncoder();
      await start(
        Object.entries(demos.hello).map(([path, text]) => ({
          path,
          data: encoder.encode(text),
        })),
      );
      const url = new URL(location.href);
      url.searchParams.delete("kapsule");
      history.replaceState(null, "", url);
    }
  });
});
$("folder").addEventListener("change", (event) => {
  unlockAudio();
  const files = [...event.target.files];
  loading(async (signal) => {
    if (!files.length) return;
    if (files.reduce((sum, file) => sum + file.size, 0) > 64 * 1024 * 1024)
      throw new Error("La Kapsule dépasse la limite de 64 Mo.");
    const imported = await Promise.all(
      files.map(async (file) => ({
        path: file.webkitRelativePath.split("/").slice(1).join("/"),
        data: new Uint8Array(await file.arrayBuffer()),
      })),
    );
    if (!signal.aborted) {
      await start(imported);
      const url = new URL(location.href);
      url.searchParams.delete("kapsule");
      history.replaceState(null, "", url);
    }
  }).finally(() => {
    event.target.value = "";
  });
});
$("stop").addEventListener("click", () =>
  guard(() => {
    module.stop();
    loadController?.abort();
    running = false;
    globalThis.kapsuleAudio.stop();
    held.clear();
    $("stop").disabled = true;
    $("zoom").disabled = true;
    status("Kapsule arrêtée.");
  }),
);
$("zoom").addEventListener("click", () => guard(toggleZoom));
const keyNames = {
  ArrowLeft: "left",
  ArrowRight: "right",
  ArrowUp: "up",
  ArrowDown: "down",
  Enter: "enter",
  Escape: "esc",
  Backspace: "backspace",
  Delete: "delete",
  Tab: "tab",
  PageUp: "pageup",
  PageDown: "pagedown",
  Home: "home",
  End: "end",
  Insert: "insert",
  " ": "space",
};
function keyName(event) {
  return (
    keyNames[event.key] ||
    (/^F\d+$/.test(event.key)
      ? event.key.toLowerCase()
      : [...event.key].length === 1
        ? `char:${event.key}`
        : null)
  );
}
canvas.addEventListener("keydown", (event) => {
  if (!running || event.metaKey) return;
  const name = keyName(event);
  if (!name) return;
  event.preventDefault();
  if (name === "f5") {
    if (!event.repeat) guard(toggleZoom);
    return;
  }
  if (event.ctrlKey && event.key.toLowerCase() === "c") {
    $("stop").click();
    return;
  }
  held.set(event.code, name);
  guard(() => call({ op: "key", name, kind: event.repeat ? 1 : 0 }));
});
canvas.addEventListener("keyup", (event) => {
  const name = held.get(event.code);
  held.delete(event.code);
  if (!running || !name) return;
  event.preventDefault();
  guard(() => call({ op: "key", name, kind: 2 }));
});
function focus(active) {
  if (!running || inDialog) return;
  if (!active) held.clear();
  lastTick = 0;
  guard(() => call({ op: "focus", active }));
}
canvas.addEventListener("blur", () => focus(false));
canvas.addEventListener("focus", () => focus(true));
window.addEventListener("blur", () => focus(false));
document.addEventListener("visibilitychange", () =>
  focus(!document.hidden && document.activeElement === canvas),
);
window.addEventListener("resize", () => {
  if (running && zoomed)
    guard(async () => paint(await call({ op: "resize", ...size() })));
});
function mouse(event, name, button) {
  if (!running || !frame) return;
  const box = canvas.getBoundingClientRect();
  const x =
      Math.floor(((event.clientX - box.left) * frame.width) / box.width) + 1,
    y = Math.floor(((event.clientY - box.top) * frame.height) / box.height) + 1;
  guard(() =>
    call({
      op: "mouse",
      event: name,
      button,
      x,
      y,
      dx: event.deltaX ? -Math.sign(event.deltaX) : 0,
      dy: event.deltaY ? -Math.sign(event.deltaY) : 0,
    }),
  );
}
const buttons = ["left", "middle", "right"];
canvas.addEventListener("pointerdown", (e) => {
  canvas.focus();
  canvas.setPointerCapture(e.pointerId);
  mouse(e, "mousepressed", buttons[e.button]);
});
canvas.addEventListener("pointerup", (e) =>
  mouse(e, "mousereleased", buttons[e.button]),
);
canvas.addEventListener("pointermove", (e) =>
  mouse(
    e,
    e.buttons ? "mousedragged" : "mousemoved",
    e.buttons & 1 ? "left" : e.buttons & 2 ? "right" : "middle",
  ),
);
canvas.addEventListener(
  "wheel",
  (e) => {
    e.preventDefault();
    mouse(e, "mousewheel");
  },
  { passive: false },
);
canvas.addEventListener("contextmenu", (e) => e.preventDefault());
for (const button of document.querySelectorAll("[data-key]")) {
  button.addEventListener("pointerdown", (e) => {
    e.preventDefault();
    if (!running) return;
    button.setPointerCapture(e.pointerId);
    canvas.focus();
    guard(() => call({ op: "key", name: button.dataset.key, kind: 0 }));
  });
  for (const event of ["pointerup", "pointercancel"])
    button.addEventListener(event, () => {
      if (running)
        guard(() => call({ op: "key", name: button.dataset.key, kind: 2 }));
    });
}
try {
  module = new BrowserEngine({ onEvent: engineEvent });
  await module.ready;
  $("launch").disabled = false;
  $("folder").disabled = false;
  $("refresh-store").disabled = false;
  status(
    "Moteur prêt. Choisissez un exemple ou ouvrez le dossier d’une Kapsule.",
  );
  requestAnimationFrame(tick);
  const id = new URL(location.href).searchParams.get("kapsule");
  await refreshStore(id ? `store:${id}` : undefined);
} catch (error) {
  status(`Chargement impossible : ${error.message}`, true);
}
