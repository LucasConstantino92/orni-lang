const fs = require("node:fs");
const path = require("node:path");
const { spawn } = require("node:child_process");

/** Runs the configured CLI or bundled WASM without requiring Cargo. */
function startCompiler(extensionPath, root, args, compilerPath) {
  const options = { cwd: root, windowsHide: true, env: { ...process.env, ELECTRON_RUN_AS_NODE: "1" } };
  if (compilerPath) return spawn(compilerPath, args, options);
  const wasm = path.join(extensionPath, "compiler", "orni.wasm");
  const runner = path.join(extensionPath, "compiler", "wasi-runner.mjs");
  if (!fs.existsSync(wasm)) throw new Error("Bundled compiler is missing. Build the extension package or configure orni.compilerPath.");
  return spawn(process.execPath, ["--no-warnings", runner, wasm, ...args], options);
}

module.exports = { startCompiler };
