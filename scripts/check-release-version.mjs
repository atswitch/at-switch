import { readFile } from "node:fs/promises";

const releaseTag = process.env.GITHUB_REF_NAME;
if (!releaseTag) {
  throw new Error("GITHUB_REF_NAME is required to verify a release.");
}

const match = /^v(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)$/.exec(releaseTag);
if (!match) {
  throw new Error(`Release tag must use semantic version format (for example v0.1.7): ${releaseTag}`);
}
const expectedVersion = match[1];

const [packageJson, packageLock, cargoToml, tauriConfig] = await Promise.all([
  readFile(new URL("../package.json", import.meta.url), "utf8").then(JSON.parse),
  readFile(new URL("../package-lock.json", import.meta.url), "utf8").then(JSON.parse),
  readFile(new URL("../src-tauri/Cargo.toml", import.meta.url), "utf8"),
  readFile(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8").then(JSON.parse),
]);

const cargoVersion = /^version\s*=\s*"([^"]+)"/m.exec(cargoToml)?.[1];
const versions = new Map([
  ["package.json", packageJson.version],
  ["package-lock.json", packageLock.version],
  ['package-lock.json packages[""]', packageLock.packages?.[""]?.version],
  ["src-tauri/Cargo.toml", cargoVersion],
  ["src-tauri/tauri.conf.json", tauriConfig.version],
]);

const mismatches = [...versions].filter(([, version]) => version !== expectedVersion);
if (mismatches.length) {
  const details = mismatches
    .map(([file, version]) => `${file}: ${version ?? "missing"}`)
    .join(", ");
  throw new Error(`Release tag ${releaseTag} does not match manifest versions: ${details}`);
}

console.log(`Release tag ${releaseTag} matches all version manifests.`);
