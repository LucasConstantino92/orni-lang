import { readFile } from "node:fs/promises";
import process from "node:process";
import { WASI } from "node:wasi";

const [wasmPath, ...programArguments] = process.argv.slice(2);

if (!wasmPath) {
  console.error("Usage: node tools/wasi-runner.mjs <module.wasm> [arguments...]");
  process.exit(2);
}

const wasi = new WASI({
  version: "preview1",
  returnOnExit: true,
  args: [wasmPath, ...programArguments],
  env: process.env,
  preopens: {
    ".": process.cwd(),
  },
});

const bytes = await readFile(wasmPath);
const module = await WebAssembly.compile(bytes);
const instance = await WebAssembly.instantiate(module, wasi.getImportObject());

process.exitCode = wasi.start(instance);
