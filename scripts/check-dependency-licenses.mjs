import { existsSync, readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

// New expressions require an explicit review before they are added here. This keeps
// dependency updates from silently introducing an unreviewed license.
const approvedNpmExpressions = new Set([
  "Apache-2.0",
  "Apache-2.0 OR MIT",
  "BSD-2-Clause",
  "BSD-3-Clause",
  "CC-BY-4.0",
  "ISC",
  "MIT",
  "MIT OR Apache-2.0",
  "MIT-0",
]);

const approvedCargoExpressions = new Set([
  "(MIT OR Apache-2.0) AND Unicode-3.0",
  "0BSD OR MIT OR Apache-2.0",
  "Apache-2.0",
  "Apache-2.0 / MIT",
  "Apache-2.0 AND ISC",
  "Apache-2.0 AND MIT",
  "Apache-2.0 OR BSL-1.0",
  "Apache-2.0 OR ISC OR MIT",
  "Apache-2.0 OR MIT",
  "Apache-2.0 WITH LLVM-exception",
  "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT",
  "Apache-2.0/MIT",
  "BSD-2-Clause OR Apache-2.0 OR MIT",
  "BSD-3-Clause",
  "BSD-3-Clause AND MIT",
  "BSD-3-Clause OR MIT OR Apache-2.0",
  "BSD-3-Clause/MIT",
  "CC0-1.0 OR MIT-0 OR Apache-2.0",
  "ISC",
  "MIT",
  "MIT AND BSD-3-Clause",
  "MIT OR Apache-2.0",
  "MIT OR Apache-2.0 OR LGPL-2.1-or-later",
  "MIT OR Apache-2.0 OR Zlib",
  "MIT OR Zlib OR Apache-2.0",
  "MIT/Apache-2.0",
  "MPL-2.0",
  "Unicode-3.0",
  "Unlicense OR MIT",
  "Unlicense/MIT",
  "Zlib",
  "Zlib OR Apache-2.0 OR MIT",
]);

function normalizeNpmLicense(value) {
  if (typeof value === "string") return value.trim();
  if (Array.isArray(value)) {
    return value
      .map((entry) => (typeof entry === "string" ? entry : entry?.type))
      .filter(Boolean)
      .join(" OR ");
  }
  if (value && typeof value.type === "string") return value.type.trim();
  return undefined;
}

function checkNpmLicenses() {
  const lock = JSON.parse(readFileSync(path.join(repositoryRoot, "package-lock.json"), "utf8"));
  const problems = [];
  let checked = 0;

  for (const [packageDirectory, lockEntry] of Object.entries(lock.packages ?? {})) {
    if (!packageDirectory) continue;
    let license = normalizeNpmLicense(lockEntry.license);
    if (!license) {
      const manifestPath = path.join(repositoryRoot, packageDirectory, "package.json");
      if (existsSync(manifestPath)) {
        const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
        license = normalizeNpmLicense(manifest.license ?? manifest.licenses);
      }
    }
    checked += 1;
    if (!license) {
      problems.push(`${packageDirectory}: missing license metadata`);
    } else if (!approvedNpmExpressions.has(license)) {
      problems.push(`${packageDirectory}: unreviewed license expression ${license}`);
    }
  }

  return { checked, problems };
}

function checkCargoLicenses() {
  const result = spawnSync(
    "cargo",
    [
      "metadata",
      "--manifest-path",
      "src-tauri/Cargo.toml",
      "--locked",
      "--format-version",
      "1",
    ],
    { cwd: repositoryRoot, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
  );
  if (result.status !== 0) {
    throw new Error(`cargo metadata failed:\n${result.stderr || result.stdout}`);
  }

  const metadata = JSON.parse(result.stdout);
  const packages = metadata.packages.filter((pkg) => pkg.source);
  const problems = packages.flatMap((pkg) => {
    if (!pkg.license) return [`${pkg.name}@${pkg.version}: missing license metadata`];
    if (!approvedCargoExpressions.has(pkg.license)) {
      return [`${pkg.name}@${pkg.version}: unreviewed license expression ${pkg.license}`];
    }
    return [];
  });
  return { checked: packages.length, problems };
}

const npm = checkNpmLicenses();
const cargo = checkCargoLicenses();
const problems = [...npm.problems, ...cargo.problems];

if (problems.length) {
  throw new Error(`Dependency license review failed:\n${problems.join("\n")}`);
}

console.log(
  `Dependency license metadata is approved (${npm.checked} npm packages, ${cargo.checked} Rust crates).`,
);
