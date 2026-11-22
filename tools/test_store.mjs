import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
const webRoot = process.env.KAPSULE_WEB_DIR;
import { fileURLToPath } from "node:url";
import {
  normalizeCatalog,
  filesFor,
  loadCatalog,
  downloadApplication,
  relativePath,
  MAX_BYTES,
} from "../web/store.js";
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const bundle = path.join(webRoot || path.join(root, "dist/web"), "store");
const snapshot = JSON.parse(
  fs.readFileSync(path.join(bundle, "catalog.json"), "utf8"),
);
const blobs = new Map();
for (const app of snapshot.applications) {
  const payload = JSON.parse(
    fs.readFileSync(path.join(bundle, snapshot.packages[app.id]), "utf8"),
  );
  const prefix = app.descriptor.slice(0, -"apps.toml".length);
  for (const file of payload.files)
    blobs.set(prefix + file.path, Buffer.from(file.data, "base64"));
}
const tree = {
  tree: snapshot.files.map((file) => ({
    ...file,
    type: "blob",
    mode: "100644",
  })),
};
const catalog = normalizeCatalog(
  {
    applications: [
      ...snapshot.applications,
      { id: "external", descriptor: "applications/helix/apps.toml" },
    ],
  },
  tree,
  snapshot.commit,
);
assert.equal(catalog.applications.length, snapshot.applications.length);
assert(
  catalog.applications.some(
    (app) =>
      app.id === "my_lua_app" && app.descriptor.includes("template_lua_app/"),
  ),
);
assert(
  filesFor(catalog, "rockland").files.some(
    (file) => file.relative === "levels.lua",
  ),
);
for (const value of [
  "../app.lua",
  "/absolute.lua",
  "folder//app.lua",
  "folder\\app.lua",
])
  assert.throws(() => relativePath(value));
assert.throws(
  () =>
    normalizeCatalog(
      { applications: snapshot.applications },
      { ...tree, truncated: true },
      snapshot.commit,
    ),
  /incomplet/,
);
assert.throws(
  () => filesFor({ ...catalog, files: [] }, "rockland"),
  /incomplets/,
);
assert.throws(
  () =>
    filesFor(
      {
        ...catalog,
        files: catalog.files.map((file) =>
          file.path.endsWith("rockland/app.lua")
            ? { ...file, size: MAX_BYTES + 1 }
            : file,
        ),
      },
      "rockland",
    ),
  /limite/,
);
const requests = [];
const mockGitHub = async (url) => {
  requests.push(url);
  if (url.endsWith("/commits/main"))
    return Response.json({ sha: snapshot.commit });
  if (url.includes("/git/trees/")) return Response.json(tree);
  if (url.endsWith("/dist/store-index.json"))
    return Response.json({ applications: snapshot.applications });
  const prefix = `https://raw.githubusercontent.com/redbug26/kkc-store/${snapshot.commit}/`;
  assert(url.startsWith(prefix), `Unpinned file request: ${url}`);
  return new Response(blobs.get(decodeURIComponent(url.slice(prefix.length))));
};
const online = await loadCatalog({ fetcher: mockGitHub });
assert.equal(online.source, "github");
assert(
  requests.some((url) =>
    url.includes(`/git/trees/${snapshot.commit}?recursive=1`),
  ),
);
for (const app of online.applications) {
  const progress = [];
  const downloaded = await downloadApplication(online, app.id, {
    fetcher: mockGitHub,
    onProgress: (done, total) => progress.push([done, total]),
  });
  const expected = filesFor(online, app.id).files;
  assert.equal(downloaded.length, expected.length);
  for (const file of downloaded) {
    const source = expected.find((item) => item.relative === file.path);
    assert.deepEqual(Buffer.from(file.data), blobs.get(source.path));
  }
  assert.deepEqual(progress.at(-1), [expected.length, expected.length]);
}
const binary = new Uint8Array([0, 255, 128, 10, 13, 0, 254]);
const binaryPath = "applications/rockland/assets/sample.bin";
const withBinary = {
  ...online,
  files: [...online.files, { path: binaryPath, size: binary.length }],
};
const binaryFiles = await downloadApplication(withBinary, "rockland", {
  fetcher: (url) =>
    url.endsWith(binaryPath) ? new Response(binary) : mockGitHub(url),
});
assert.deepEqual(
  binaryFiles.find((file) => file.path === "assets/sample.bin").data,
  binary,
);
const offlineFetch = async (url) => {
  if (url.startsWith("https://"))
    return new Response("rate limited", { status: 403 });
  if (url === "./store/catalog.json") return Response.json(snapshot);
  assert(url.startsWith("./store/packages/"));
  return new Response(
    fs.readFileSync(
      path.join(bundle, decodeURIComponent(url.slice("./store/".length))),
    ),
  );
};
const offline = await loadCatalog({ fetcher: offlineFetch });
assert.equal(offline.source, "bundled");
assert.equal(
  (await downloadApplication(offline, "my_lua_app", { fetcher: offlineFetch }))
    .length,
  filesFor(offline, "my_lua_app").files.length,
);
await assert.rejects(
  downloadApplication(online, "rockland", {
    fetcher: async () => new Response("broken", { status: 404 }),
  }),
  /HTTP 404/,
);
await assert.rejects(
  downloadApplication(online, "rockland", {
    fetcher: async () => new Response("truncated"),
  }),
  /incomplet/,
);
const controller = new AbortController();
const pending = downloadApplication(online, "rockland", {
  signal: controller.signal,
  fetcher: (_url, { signal }) =>
    new Promise((_resolve, reject) =>
      signal.addEventListener("abort", () => reject(signal.reason), {
        once: true,
      }),
    ),
});
controller.abort();
await assert.rejects(pending, (error) => error.name === "AbortError");
if (process.argv.includes("--live")) {
  const live = await loadCatalog();
  assert.equal(live.source, "github");
  const files = await downloadApplication(live, "rockland");
  assert(
    files.some((file) => file.path === "app.lua") &&
      files.some((file) => file.path === "levels.lua"),
  );
  console.log(
    `Live GitHub store: ${live.applications.length} Kapsules, Rockland and its modules downloaded at ${live.commit.slice(0, 12)}`,
  );
}
console.log(
  `Store: ${catalog.applications.length} Kapsules, pinned downloads, binary-safe assets, template IDs, offline fallback, limits and cancellation passed`,
);
