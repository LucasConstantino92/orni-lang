import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
execFileSync("cargo", ["build", "--locked", "--release", "--target", "wasm32-wasip1"], { cwd: root, stdio: "inherit" });
const destination = path.join(root, "editors", "vscode", "compiler");
mkdirSync(destination, { recursive: true });
copyFileSync(path.join(root, "target", "wasm32-wasip1", "release", "orni.wasm"), path.join(destination, "orni.wasm"));
copyFileSync(path.join(root, "tools", "wasi-runner.mjs"), path.join(destination, "wasi-runner.mjs"));
