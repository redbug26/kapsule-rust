import { MAX_BYTES, relativePath } from "./store.js";
export function validateFiles(files) {
  if (!Array.isArray(files) || !files.length || files.length > 4096)
    throw new Error("Une Kapsule doit contenir entre 1 et 4096 fichiers.");
  const paths = new Set();
  let bytes = 0;
  for (const file of files) {
    relativePath(file.path);
    if (
      file.path.length > 1024 ||
      paths.has(file.path) ||
      !(file.data instanceof Uint8Array)
    )
      throw new Error("Fichier ou chemin dupliqué invalide dans la Kapsule.");
    paths.add(file.path);
    bytes += file.data.byteLength;
    if (bytes > MAX_BYTES)
      throw new Error("La Kapsule dépasse la limite de 64 Mo.");
  }
  for (const name of paths) {
    const parts = name.split("/");
    parts.pop();
    while (parts.length) {
      if (paths.has(parts.join("/")))
        throw new Error("Un fichier et un dossier utilisent le même chemin.");
      parts.pop();
    }
  }
  if (!paths.has("apps.toml"))
    throw new Error("Le dossier doit contenir apps.toml à sa racine.");
  return files;
}
