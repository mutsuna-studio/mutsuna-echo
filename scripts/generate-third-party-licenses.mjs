import { createHash } from "node:crypto";
import { execSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const checkOnly = process.argv.includes("--check");
const licenseFilePattern = /^(?:licen[cs]e|copying|notice)(?:[._-].*)?$/i;
const textExtensions = new Set(["", ".txt", ".md", ".rst"]);

function command(commandLine) {
  return execSync(commandLine, {
    cwd: repositoryRoot,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    stdio: ["ignore", "pipe", "pipe"]
  });
}

function normalizeText(value) {
  return value.replaceAll("\r\n", "\n").trim() + "\n";
}

function readText(path) {
  return normalizeText(readFileSync(path, "utf8"));
}

function noticeFiles(packageDirectory, explicitLicenseFile) {
  const candidates = new Set();
  if (explicitLicenseFile) {
    const path = resolve(packageDirectory, explicitLicenseFile);
    if (existsSync(path)) candidates.add(path);
  }
  if (existsSync(packageDirectory)) {
    for (const entry of readdirSync(packageDirectory, { withFileTypes: true })) {
      if (!entry.isFile() || !licenseFilePattern.test(entry.name)) continue;
      const extension = entry.name.includes(".") ? entry.name.slice(entry.name.lastIndexOf(".")).toLowerCase() : "";
      if (textExtensions.has(extension) || entry.name.toLowerCase().startsWith("license-")) {
        candidates.add(join(packageDirectory, entry.name));
      }
    }
  }
  return [...candidates].sort().map((path) => ({
    name: relative(packageDirectory, path).replaceAll("\\", "/"),
    text: readText(path)
  }));
}

function authorLabel(value) {
  if (Array.isArray(value)) return value.join(", ");
  if (typeof value === "string") return value;
  if (value && typeof value === "object") return value.name ?? value.email ?? "";
  return "";
}

const genericMit = readText(join(repositoryRoot, "third-party", "onnxruntime", "LICENSE.txt"));
const exactFallbackFiles = new Map([
  ["Rust\0alloc-stdlib", "third-party/rust/alloc-stdlib-LICENSE.txt"],
  ["Rust\0sonora-aec3", "third-party/rust/sonora-aec3-LICENSE.txt"]
]);
function fallbackDocuments(component) {
  const exactFile = exactFallbackFiles.get(`${component.ecosystem}\0${component.name}`);
  if (exactFile) return [fileDocument(exactFile, "LICENSE.txt")];
  if (/MPL-2\.0/.test(component.license)) {
    return [{ name: "MPL-2.0.txt", text: readText(join(repositoryRoot, "LICENSE")) }];
  }
  if (/Apache-2\.0/.test(component.license)) {
    return [{ name: "Apache-2.0.txt", text: readText(join(repositoryRoot, "src-tauri", "vendor", "sherpa-onnx", "LICENSE")) }];
  }
  if (/\bMIT\b/.test(component.license)) {
    const owner = component.author || component.name;
    return [{
      name: "MIT.txt",
      text: genericMit.replace("Copyright (c) Microsoft Corporation", `Copyright (c) ${owner}`)
    }];
  }
  if (/\bISC\b/.test(component.license)) {
    const owner = component.author || component.name;
    return [{
      name: "ISC.txt",
      text: `ISC License\n\nCopyright (c) ${owner}\n\nPermission to use, copy, modify, and/or distribute this software for any\npurpose with or without fee is hereby granted, provided that the above\ncopyright notice and this permission notice appear in all copies.\n\nTHE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES\nWITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF\nMERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR\nANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES\nWHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN\nACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF\nOR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.\n`
    }];
  }
  if (/BSD-3-Clause/.test(component.license)) {
    const owner = component.author || component.name;
    return [{
      name: "BSD-3-Clause.txt",
      text: `Copyright (c) ${owner}\nAll rights reserved.\n\nRedistribution and use in source and binary forms, with or without modification, are permitted provided that the following conditions are met:\n\n1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following disclaimer.\n\n2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the following disclaimer in the documentation and/or other materials provided with the distribution.\n\n3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote products derived from this software without specific prior written permission.\n\nTHIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.\n`
    }];
  }
  return [];
}

function npmComponents() {
  const groups = JSON.parse(command("pnpm licenses list --prod --json"));
  const result = [];
  for (const [license, packages] of Object.entries(groups)) {
    for (const packageInfo of packages) {
      packageInfo.versions.forEach((version, index) => {
        const directory = packageInfo.paths[index] ?? packageInfo.paths[0];
        const component = {
          ecosystem: "npm",
          name: packageInfo.name,
          version,
          license,
          sourceUrl: packageInfo.homepage || `https://www.npmjs.com/package/${encodeURIComponent(packageInfo.name)}`,
          author: authorLabel(packageInfo.author),
          documents: noticeFiles(directory)
        };
        if (component.documents.length === 0) component.documents = fallbackDocuments(component);
        result.push(component);
      });
    }
  }
  return result;
}

function cargoComponents() {
  const metadata = JSON.parse(command("cargo metadata --format-version 1 --manifest-path src-tauri/Cargo.toml"));
  return metadata.packages
    .filter((packageInfo) => packageInfo.name !== "mutsuna-echo")
    .map((packageInfo) => {
      const directory = dirname(packageInfo.manifest_path);
      const component = {
        ecosystem: "Rust",
        name: packageInfo.name,
        version: packageInfo.version,
        license: packageInfo.license || "See bundled license",
        sourceUrl: packageInfo.repository || `https://crates.io/crates/${encodeURIComponent(packageInfo.name)}/${packageInfo.version}`,
        author: authorLabel(packageInfo.authors),
        documents: noticeFiles(directory, packageInfo.license_file)
      };
      if (component.documents.length === 0) component.documents = fallbackDocuments(component);
      return component;
    });
}

function fileDocument(path, name) {
  return { name, text: readText(join(repositoryRoot, path)) };
}

function curatedComponents() {
  const apache = () => [fileDocument("src-tauri/vendor/sherpa-onnx/LICENSE", "Apache-2.0.txt")];
  return [
    {
      ecosystem: "Native runtime", name: "sherpa-onnx", version: "1.13.4", license: "Apache-2.0",
      sourceUrl: "https://github.com/k2-fsa/sherpa-onnx", author: "k2-fsa", documents: apache()
    },
    {
      ecosystem: "Native runtime", name: "ONNX Runtime", version: "1.27.0", license: "MIT",
      sourceUrl: "https://github.com/microsoft/onnxruntime/tree/v1.27.0", author: "Microsoft Corporation",
      documents: [
        fileDocument("third-party/onnxruntime/LICENSE.txt", "LICENSE.txt"),
        fileDocument("third-party/onnxruntime/ThirdPartyNotices.txt", "ThirdPartyNotices.txt")
      ]
    },
    {
      ecosystem: "AI model", name: "ReazonSpeech K2 v2", version: "2024-08-01", license: "Apache-2.0",
      sourceUrl: "https://huggingface.co/reazon-research/reazonspeech-k2-v2/tree/291488c8151be24d7da4bf7af26e533fad96e407",
      author: "Reazon Human Interaction Lab", documents: apache()
    },
    {
      ecosystem: "AI model", name: "Silero VAD", version: "5.0", license: "MIT",
      sourceUrl: "https://github.com/snakers4/silero-vad/tree/v5.0", author: "Silero Team",
      documents: [fileDocument("third-party/models/silero-vad-LICENSE.txt", "LICENSE.txt")]
    },
    {
      ecosystem: "AI model", name: "pyannote segmentation", version: "3.0", license: "MIT",
      sourceUrl: "https://huggingface.co/pyannote/segmentation-3.0", author: "CNRS",
      documents: [fileDocument("third-party/models/pyannote-segmentation-LICENSE.txt", "LICENSE.txt")]
    },
    {
      ecosystem: "AI model", name: "3D-Speaker ERes2Net", version: "2024-10-14", license: "Apache-2.0",
      sourceUrl: "https://github.com/modelscope/3D-Speaker", author: "3D-Speaker contributors", documents: apache()
    },
    ...[
      ["AndroidX", "https://github.com/androidx/androidx"],
      ["Kotlin", "https://github.com/JetBrains/kotlin"],
      ["Material Components for Android", "https://github.com/material-components/material-components-android"],
      ["AndroidX Media3", "https://github.com/androidx/media"],
      ["Jackson", "https://github.com/FasterXML/jackson"],
      ["Guava", "https://github.com/google/guava"]
    ].map(([name, sourceUrl]) => ({
      ecosystem: "Android", name, version: "See Gradle lock resolution", license: "Apache-2.0",
      sourceUrl, author: `${name} contributors`, documents: apache()
    })),
    {
      ecosystem: "Android", name: "Google Play Core libraries", version: "2.1.0", license: "Play Core SDK Terms of Service",
      sourceUrl: "https://developer.android.com/guide/playcore/license", author: "Google LLC",
      documents: [{
        name: "TERMS.txt",
        text: "Google Play Core libraries are used under the Play Core Software Development Kit Terms of Service.\nCurrent terms: https://developer.android.com/guide/playcore/license\n"
      }]
    }
  ];
}

function buildCatalog() {
  const documents = new Map();
  const componentMap = new Map();
  const rawComponents = [...npmComponents(), ...cargoComponents(), ...curatedComponents()];
  for (const component of rawComponents) {
    const documentIds = [];
    for (const document of component.documents) {
      const text = normalizeText(document.text);
      const id = createHash("sha256").update(text).digest("hex").slice(0, 20);
      if (!documents.has(id)) documents.set(id, { id, name: document.name, text });
      documentIds.push(id);
    }
    const key = `${component.ecosystem}\0${component.name}\0${component.version}`;
    componentMap.set(key, {
      ecosystem: component.ecosystem,
      name: component.name,
      version: component.version,
      license: component.license,
      sourceUrl: component.sourceUrl,
      documentIds: [...new Set(documentIds)]
    });
  }
  const components = [...componentMap.values()].sort((left, right) =>
    left.ecosystem.localeCompare(right.ecosystem, "en")
      || left.name.localeCompare(right.name, "en")
      || left.version.localeCompare(right.version, "en")
  );
  const projectLicenseText = readText(join(repositoryRoot, "LICENSE"));
  const projectLicenseDocumentId = createHash("sha256")
    .update(projectLicenseText)
    .digest("hex")
    .slice(0, 20);
  if (!documents.has(projectLicenseDocumentId)) {
    documents.set(projectLicenseDocumentId, {
      id: projectLicenseDocumentId,
      name: "MPL-2.0.txt",
      text: projectLicenseText
    });
  }
  return {
    schemaVersion: 1,
    project: {
      name: "Mutsuna Echo",
      license: "MPL-2.0",
      sourceUrl: "https://github.com/mutsuna-studio/mutsuna-echo",
      licenseDocumentId: projectLicenseDocumentId
    },
    components,
    documents: [...documents.values()].sort((left, right) => left.id.localeCompare(right.id))
  };
}

function noticesText(catalog) {
  const lines = [
    "Mutsuna Echo — Open Source Licenses and Third-Party Notices",
    "============================================================",
    "",
    "Mutsuna Echo is licensed under the Mozilla Public License 2.0.",
    `Source code: ${catalog.project.sourceUrl}`,
    "",
    "Third-party components",
    "----------------------",
    ""
  ];
  for (const component of catalog.components) {
    lines.push(`${component.name} ${component.version} [${component.ecosystem}]`);
    lines.push(`License: ${component.license}`);
    lines.push(`Source: ${component.sourceUrl}`);
    if (component.documentIds.length > 0) lines.push(`License documents: ${component.documentIds.join(", ")}`);
    lines.push("");
  }
  lines.push("License documents", "-----------------", "");
  for (const document of catalog.documents) {
    lines.push(`Document ${document.id}: ${document.name}`, "", document.text.trimEnd(), "");
  }
  return lines.join("\n") + "\n";
}

const catalog = buildCatalog();
const outputs = new Map([
  [join(repositoryRoot, "static", "third-party-licenses.json"), JSON.stringify(catalog, null, 2) + "\n"],
  [join(repositoryRoot, "static", "THIRD-PARTY-NOTICES.txt"), noticesText(catalog)],
  [join(repositoryRoot, "static", "MUTSUNA-ECHO-LICENSE.txt"), readText(join(repositoryRoot, "LICENSE"))]
]);

for (const [path, content] of outputs) {
  if (checkOnly) {
    const existing = await readFile(path, "utf8").catch(() => "");
    if (normalizeText(existing) !== normalizeText(content)) {
      console.error(`${relative(repositoryRoot, path)} is out of date. Run pnpm licenses:generate.`);
      process.exitCode = 1;
    }
  } else {
    await mkdir(dirname(path), { recursive: true });
    await writeFile(path, content);
    console.log(`wrote ${relative(repositoryRoot, path)}`);
  }
}

if (!checkOnly) {
  console.log(`${catalog.components.length} components, ${catalog.documents.length} unique notice documents`);
}
