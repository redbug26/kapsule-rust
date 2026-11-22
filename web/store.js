// Public KKC store loader. No account, token, proxy or application server is needed.
export const STORE_REPO = "redbug26/kkc-store";
export const STORE_URL = `https://github.com/${STORE_REPO}`;
const API = `https://api.github.com/repos/${STORE_REPO}`;
const RAW = `https://raw.githubusercontent.com/${STORE_REPO}`;
export const MAX_BYTES = 64 * 1024 * 1024;
const MAX_FILES = 4096;

export function relativePath(value) {
  if (
    typeof value !== "string" ||
    !value ||
    value.length > 1024 ||
    value.includes("\\") ||
    value.includes("\0") ||
    value.split("/").some((part) => !part || part === "." || part === "..")
  )
    throw new Error("Chemin invalide dans le store.");
  return value;
}
function application(entry) {
  if (
    !entry ||
    typeof entry.runtime !== "object" ||
    !entry.runtime ||
    Array.isArray(entry.runtime)
  )
    return null;
  if (typeof entry.id !== "string" || !entry.id.trim() || entry.id.length > 128)
    throw new Error("Identifiant invalide dans le store.");
  const descriptor = relativePath(entry.descriptor);
  if (
    !descriptor.startsWith("applications/") ||
    !descriptor.endsWith("/apps.toml")
  )
    throw new Error("Descripteur de Kapsule invalide dans le store.");
  return {
    id: entry.id,
    name: entry.name || entry.id,
    version: entry.version || "",
    description: entry.description || "",
    descriptor,
    runtime: entry.runtime,
  };
}
export function normalizeCatalog(index, tree, commit) {
  if (
    !/^[0-9a-f]{40}$/.test(commit) ||
    !Array.isArray(index?.applications) ||
    !Array.isArray(tree?.tree) ||
    tree.truncated
  )
    throw new Error("Catalogue GitHub incomplet ou invalide.");
  const applications = index.applications.map(application).filter(Boolean);
  if (
    !applications.length ||
    new Set(applications.map((app) => app.id)).size !== applications.length
  )
    throw new Error("Catalogue de Kapsules vide ou ambigu.");
  const roots = applications.map((app) =>
    app.descriptor.slice(0, -"apps.toml".length),
  );
  const files = tree.tree
    .filter(
      (file) =>
        file.type === "blob" &&
        roots.some((root) => file.path.startsWith(root)),
    )
    .map((file) => {
      relativePath(file.path);
      if (
        !["100644", "100755"].includes(file.mode) ||
        !Number.isSafeInteger(file.size) ||
        file.size < 0
      )
        throw new Error("Type de fichier non pris en charge dans le store.");
      return { path: file.path, size: file.size };
    });
  if (new Set(files.map((file) => file.path)).size !== files.length)
    throw new Error("Fichiers dupliqués dans le store.");
  const catalog = {
    commit,
    applications: applications.sort((a, b) => a.name.localeCompare(b.name)),
    files,
  };
  for (const app of applications) filesFor(catalog, app.id);
  return catalog;
}
export function filesFor(catalog, id) {
  const app = catalog.applications.find((item) => item.id === id);
  if (!app) throw new Error("Kapsule absente du store.");
  const root = relativePath(app.descriptor).slice(0, -"apps.toml".length);
  const files = catalog.files
    .filter((file) => file.path.startsWith(root))
    .map((file) => ({
      ...file,
      relative: relativePath(file.path.slice(root.length)),
    }));
  if (
    !files.some((file) => file.relative === "apps.toml") ||
    !files.some((file) => file.relative === (app.runtime.main || "app.lua"))
  )
    throw new Error(`Fichiers Lua incomplets pour ${app.name}.`);
  if (
    files.length > MAX_FILES ||
    files.reduce((total, file) => total + file.size, 0) > MAX_BYTES
  )
    throw new Error(
      "Cette Kapsule dépasse la limite de 64 Mo ou de 4096 fichiers.",
    );
  return { app, files };
}
function encoded(path) {
  return relativePath(path).split("/").map(encodeURIComponent).join("/");
}
async function fetchResponse(url, { signal, fetcher }) {
  const response = await fetcher(url, {
    signal: signal
      ? AbortSignal.any([signal, AbortSignal.timeout(15000)])
      : AbortSignal.timeout(15000),
    cache: "no-cache",
    credentials: "omit",
  });
  if (!response.ok)
    throw new Error(
      response.status === 403 || response.status === 429
        ? "GitHub limite temporairement les téléchargements."
        : `Téléchargement impossible (HTTP ${response.status}).`,
    );
  return response;
}
async function boundedJSON(response, limit = 8 * 1024 * 1024) {
  const chunks = [];
  let size = 0;
  const reader = response.body?.getReader();
  if (!reader) {
    const data = new Uint8Array(await response.arrayBuffer());
    if (data.length > limit)
      throw new Error("Réponse du store trop volumineuse.");
    return JSON.parse(new TextDecoder().decode(data));
  }
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      size += value.byteLength;
      if (size > limit) {
        await reader.cancel();
        throw new Error("Réponse du store trop volumineuse.");
      }
      chunks.push(value);
    }
  } finally {
    reader.releaseLock();
  }
  const data = new Uint8Array(size);
  let offset = 0;
  for (const chunk of chunks) {
    data.set(chunk, offset);
    offset += chunk.length;
  }
  return JSON.parse(new TextDecoder().decode(data));
}
export async function loadCatalog({
  signal,
  fetcher = fetch,
  bundled = "./store/catalog.json",
} = {}) {
  const options = { signal, fetcher };
  try {
    const revision = await boundedJSON(
      await fetchResponse(`${API}/commits/main`, options),
    );
    if (!/^[0-9a-f]{40}$/.test(revision.sha))
      throw new Error("Révision GitHub invalide.");
    const [index, tree] = await Promise.all([
      fetchResponse(
        `${RAW}/${revision.sha}/dist/store-index.json`,
        options,
      ).then((r) => boundedJSON(r)),
      fetchResponse(
        `${API}/git/trees/${revision.sha}?recursive=1`,
        options,
      ).then((r) => boundedJSON(r)),
    ]);
    return { ...normalizeCatalog(index, tree, revision.sha), source: "github" };
  } catch (error) {
    if (signal?.aborted) throw error;
    const snapshot = await boundedJSON(await fetchResponse(bundled, options));
    if (
      snapshot.format !== 2 ||
      !snapshot.packages ||
      typeof snapshot.packages !== "object"
    )
      throw new Error("Copie embarquée du store invalide.");
    const catalog = normalizeCatalog(
      { applications: snapshot.applications },
      {
        tree: snapshot.files.map((f) => ({
          ...f,
          type: "blob",
          mode: "100644",
        })),
      },
      snapshot.commit,
    );
    for (const app of catalog.applications)
      if (!/^packages\/[0-9a-f]{64}\.json$/.test(snapshot.packages[app.id]))
        throw new Error("Paquet de Kapsule invalide.");
    return { ...catalog, packages: snapshot.packages, source: "bundled" };
  }
}
async function bundledApplication(catalog, id, files, options) {
  const packagePath = catalog.packages?.[id];
  if (!/^packages\/[0-9a-f]{64}\.json$/.test(packagePath))
    throw new Error("Paquet de Kapsule invalide.");
  const payload = await boundedJSON(
    await fetchResponse(`${options.bundled}${packagePath}`, options),
    96 * 1024 * 1024,
  );
  if (
    payload.id !== id ||
    payload.commit !== catalog.commit ||
    !Array.isArray(payload.files) ||
    payload.files.length !== files.length
  )
    throw new Error("Identité du paquet de Kapsule invalide.");
  const expected = new Map(files.map((file) => [file.relative, file])),
    seen = new Set(),
    result = [];
  for (const item of payload.files) {
    if (options.signal?.aborted) throw options.signal.reason;
    const file = expected.get(relativePath(item.path));
    if (
      !file ||
      seen.has(item.path) ||
      typeof item.data !== "string" ||
      item.data.length !== 4 * Math.ceil(file.size / 3)
    )
      throw new Error("Fichier de paquet invalide.");
    const bytes = atob(item.data);
    if (bytes.length !== file.size)
      throw new Error("Taille de fichier de paquet invalide.");
    result.push({
      path: item.path,
      data: Uint8Array.from(bytes, (ch) => ch.charCodeAt(0)),
    });
    seen.add(item.path);
    options.onProgress(result.length, files.length, options.app);
  }
  return result;
}
export async function downloadApplication(
  catalog,
  id,
  { signal, fetcher = fetch, onProgress = () => {}, bundled = "./store/" } = {},
) {
  const abort = new AbortController();
  signal = signal ? AbortSignal.any([signal, abort.signal]) : abort.signal;
  const { app, files } = filesFor(catalog, id);
  onProgress(0, files.length, app);
  if (catalog.source === "bundled")
    return bundledApplication(catalog, id, files, {
      signal,
      fetcher,
      onProgress,
      bundled,
      app,
    });
  const output = new Array(files.length);
  let next = 0,
    completed = 0,
    totalBytes = 0;
  async function worker() {
    while (next < files.length) {
      const index = next++,
        file = files[index];
      const response = await fetchResponse(
        `${RAW}/${catalog.commit}/${encoded(file.path)}`,
        { signal, fetcher },
      );
      const chunks = [];
      let size = 0;
      const reader = response.body?.getReader();
      if (reader) {
        try {
          while (true) {
            const { done, value } = await reader.read();
            if (done) break;
            size += value.length;
            totalBytes += value.length;
            if (totalBytes > MAX_BYTES) {
              await reader.cancel();
              throw new Error("La Kapsule dépasse la limite de 64 Mo.");
            }
            chunks.push(value);
          }
        } finally {
          reader.releaseLock();
        }
      } else {
        const data = new Uint8Array(await response.arrayBuffer());
        size = data.length;
        totalBytes += size;
        if (totalBytes > MAX_BYTES)
          throw new Error("La Kapsule dépasse la limite de 64 Mo.");
        chunks.push(data);
      }
      const data = new Uint8Array(size);
      let offset = 0;
      for (const chunk of chunks) {
        data.set(chunk, offset);
        offset += chunk.length;
      }
      if (signal.aborted) throw signal.reason;
      if (size !== file.size)
        throw new Error(`Fichier incomplet dans le store : ${file.relative}.`);
      output[index] = { path: file.relative, data };
      onProgress(++completed, files.length, app);
    }
  }
  try {
    await Promise.all(
      Array.from({ length: Math.min(4, files.length) }, worker),
    );
  } catch (error) {
    abort.abort();
    throw error;
  }
  return output;
}
