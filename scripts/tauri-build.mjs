import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const tauriCli = fileURLToPath(
  new URL("../node_modules/@tauri-apps/cli/tauri.js", import.meta.url),
);
const rawArgs = process.argv.slice(2);
// `npm run tauri -- build ...` forwards `build` as the first arg alongside the
// `tauri` script name. The wrapper script already hardcodes `build`, so drop
// the redundant leading subcommand to avoid Tauri's CLI passing `build` through
// to the underlying `cargo build` invocation.
const forwardedArgs =
  rawArgs[0] === "build" ? rawArgs.slice(1) : rawArgs;
const macosCiArgs =
  process.platform === "darwin" && !forwardedArgs.includes("--ci")
    ? ["--ci"]
    : [];
const environment =
  process.platform === "darwin"
    ? { ...process.env, CI: "true" }
    : process.env;

const result = spawnSync(
  process.execPath,
  [tauriCli, "build", ...macosCiArgs, ...forwardedArgs],
  {
    cwd: process.cwd(),
    env: environment,
    stdio: "inherit",
  },
);

if (result.error) {
  console.error(`Unable to start the Tauri build: ${result.error.message}`);
  process.exit(1);
}

process.exit(result.status ?? 1);
