import { readFile } from "node:fs/promises";

const tag = process.argv[2] ?? process.env.GITHUB_REF_NAME;

if (!tag?.startsWith("v")) {
  throw new Error(`Release tag must start with "v" (received: ${tag ?? "nothing"})`);
}

const expectedVersion = tag.slice(1);
const packageJson = JSON.parse(await readFile("package.json", "utf8"));
const tauriConfig = JSON.parse(await readFile("src-tauri/tauri.conf.json", "utf8"));
const cargoToml = await readFile("src-tauri/Cargo.toml", "utf8");
const cargoVersion = cargoToml.match(/^version\s*=\s*"([^"]+)"/m)?.[1];

const versions = new Map([
  ["package.json", packageJson.version],
  ["src-tauri/tauri.conf.json", tauriConfig.version],
  ["src-tauri/Cargo.toml", cargoVersion],
]);

const mismatches = [...versions].filter(([, version]) => version !== expectedVersion);

if (mismatches.length > 0) {
  const details = mismatches
    .map(([file, version]) => `${file}: ${version ?? "version not found"}`)
    .join("\n");
  throw new Error(`Release tag ${tag} does not match:\n${details}`);
}

console.log(`Release versions match ${tag}.`);
