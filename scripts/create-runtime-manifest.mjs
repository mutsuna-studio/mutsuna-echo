import { createHash } from "node:crypto";
import { readdir, readFile, stat, writeFile } from "node:fs/promises";
import { join, relative } from "node:path";

const [directory, target, runtimeVersion = "1.13.4-1"] = process.argv.slice(2);
if (!directory || !target) throw new Error("usage: create-runtime-manifest.mjs <directory> <target> [version]");

async function filesBelow(root, current = root) {
  const result = [];
  for (const entry of await readdir(current, { withFileTypes: true })) {
    const path = join(current, entry.name);
    if (entry.isDirectory()) result.push(...await filesBelow(root, path));
    else if (entry.isFile() && entry.name !== "manifest.json") result.push(path);
  }
  return result;
}

const files = [];
for (const path of (await filesBelow(directory)).sort()) {
  const bytes = await readFile(path);
  files.push({
    path: relative(directory, path).replaceAll("\\", "/"),
    sizeBytes: (await stat(path)).size,
    sha256: createHash("sha256").update(bytes).digest("hex")
  });
}
if (files.length === 0) throw new Error("runtime directory is empty");
await writeFile(join(directory, "manifest.json"), JSON.stringify({
  schemaVersion: 1,
  protocolVersion: 1,
  runtimeVersion,
  target,
  files
}, null, 2) + "\n");
