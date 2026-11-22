// A hung or exhausted Lua VM can always be terminated without blocking the UI.
export class BrowserEngine {
  constructor({
    createWorker = (url) => new Worker(url, { type: "module" }),
    onEvent = () => {},
    timeout = 5000,
    startupTimeout = 15000,
  } = {}) {
    this.createWorker = createWorker;
    this.onEvent = onEvent;
    this.timeout = timeout;
    this.startupTimeout = startupTimeout;
    this.sequence = 0;
    this.pending = new Map();
    this.loaded = false;
    this.spawn();
  }
  spawn() {
    this.broken = false;
    const worker = this.createWorker(
      new URL("./engine-worker.js", import.meta.url),
    );
    this.worker = worker;
    this.ready = new Promise((resolve, reject) => {
      this.resolveReady = resolve;
      this.rejectReady = reject;
    });
    this.ready.catch(() => {});
    this.startupTimer = setTimeout(
      () =>
        this.fail(
          new Error("Le moteur met trop de temps à démarrer. Réessayez."),
        ),
      this.startupTimeout,
    );
    worker.addEventListener("error", (event) => {
      if (this.worker === worker)
        this.fail(
          new Error(
            event.message || "Le moteur WebAssembly a rencontré une erreur.",
          ),
        );
    });
    worker.addEventListener("message", ({ data }) => {
      if (this.worker !== worker) return;
      if (data.type === "ready") {
        clearTimeout(this.startupTimer);
        this.resolveReady();
        return;
      }
      if (data.type === "fatal") {
        this.fail(new Error(data.error));
        return;
      }
      if (data.type === "response") {
        const request = this.pending.get(data.id);
        if (!request) return;
        clearTimeout(request.timer);
        this.pending.delete(data.id);
        if (data.error) request.reject(new Error(data.error));
        else request.resolve(data.result);
        return;
      }
      if (data.type === "dialog") {
        for (const request of this.pending.values())
          clearTimeout(request.timer);
        try {
          this.onEvent(data);
        } finally {
          for (const request of this.pending.values()) this.arm(request);
        }
      } else this.onEvent(data);
    });
  }
  arm(request) {
    request.timer = setTimeout(
      () =>
        this.fail(
          new Error(
            "Cette Kapsule ne répond plus. Le moteur a été arrêté ; vous pouvez en lancer une autre.",
          ),
        ),
      this.timeout,
    );
  }
  fail(error) {
    clearTimeout(this.startupTimer);
    this.broken = true;
    this.worker?.terminate();
    this.worker = null;
    this.rejectReady(error);
    for (const request of this.pending.values()) {
      clearTimeout(request.timer);
      request.reject(error);
    }
    this.pending.clear();
  }
  reset() {
    const cancelled = new Error("Chargement annulé.");
    cancelled.name = "AbortError";
    this.fail(cancelled);
    this.loaded = false;
    this.spawn();
    return this.ready;
  }
  async request(message, transfers = []) {
    if (this.broken)
      throw new Error("Le moteur doit être redémarré. Cliquez sur Lancer.");
    const worker = this.worker;
    await this.ready;
    if (this.worker !== worker) {
      const error = new Error("Chargement annulé.");
      error.name = "AbortError";
      throw error;
    }
    return new Promise((resolve, reject) => {
      const id = ++this.sequence,
        request = { resolve, reject, timer: null };
      this.pending.set(id, request);
      this.arm(request);
      try {
        worker.postMessage({ ...message, id }, transfers);
      } catch (error) {
        clearTimeout(request.timer);
        this.pending.delete(id);
        reject(error);
      }
    });
  }
  async load(files, args, width, height, expectedId) {
    if (this.loaded || this.broken) await this.reset();
    this.loaded = true;
    return this.request(
      { type: "load", files, args, width, height, expectedId },
      files.map((file) => file.data.buffer),
    );
  }
  call(request) {
    return this.request({ type: "call", request });
  }
  stop() {
    return this.reset();
  }
  audioStatus(playing) {
    this.worker?.postMessage({ type: "audio-status", playing });
  }
}
