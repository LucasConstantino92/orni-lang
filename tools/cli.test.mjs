import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, writeFileSync, mkdirSync, rmSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const wasm = path.resolve(process.env.ORNI_WASM || path.join(root, "target/wasm32-wasip1/debug/orni.wasm"));
const runner = path.join(root, "tools/wasi-runner.mjs");
function run(args, cwd = root, input) {
  return spawnSync(process.execPath, ["--no-warnings", runner, wasm, ...args], {
    cwd, input, encoding: "utf8", timeout: 15000,
  });
}

test("CLI propagates success, semantic errors, runtime errors, and usage failures", () => {
  assert.equal(run(["run", "examples/hello.orni"]).status, 0);
  assert.equal(run(["check", "examples/errors/01_undefined_variable.orni"]).status, 1);
  assert.equal(run(["run", "examples/errors/05_division_by_zero.orni"]).status, 1);
  assert.equal(run(["unknown-command"]).status, 2);
});

test("the WASI runner explains that HTTP serving requires the native target", () => {
  const result = run(["serve", "examples/experimental/http_server"]);
  assert.equal(result.status, 2, result.stderr);
  assert.match(result.stderr, /requires the native Rust target/);
});

test("nested project modules validate independently and execute through their manifest", () => {
  const result = run(["check-module", "examples/experimental/framework_project/providers/console_audit.orni"]);
  assert.equal(result.status, 0, result.stderr);
  const application = run(["run", "examples/experimental/framework_project"]);
  assert.equal(application.status, 0, application.stderr);
  assert.match(application.stdout, /Orni modular application started/);
});

test("projects run outside the compiler checkout and honor runtime limits", () => {
  const directory = mkdtempSync(path.join(os.tmpdir(), "orni-cli-"));
  try {
    writeFileSync(path.join(directory, "orni.toml"), '[project]\nname = "external"\nentry = "src/main.orni"\nsource = "src"\n[runtime]\nmax_output_bytes = 3\n');
    mkdirSync(path.join(directory, "src"));
    writeFileSync(path.join(directory, "src/main.orni"), 'print "long output";');
    const result = run(["run"], directory);
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stderr, /configured limit of 3/);
    writeFileSync(path.join(directory, "orni.toml"), '[project]\nsource = "../escape"\n');
    assert.equal(run(["check"], directory).status, 1);
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

test("unsaved module overlays are checked without touching saved source", () => {
  const result = run(["--check-overlay", "examples/experimental/framework_project/main.orni", "examples/experimental/framework_project/providers/console_logger.orni"], root,
    'import contracts.logger.{Logger};\nprovider ConsoleLogger for Logger { func log(message: string) -> unit { print unknown_value; } }');
  assert.equal(result.status, 1, result.stderr);
  assert.match(result.stderr, /console_logger.orni/);
  assert.match(result.stderr, /unknown_value/);
  assert.equal(run(["check", "examples/experimental/framework_project/main.orni"]).status, 0);
});

test("module buffer analysis checks unsaved source without requiring application bindings", () => {
  const module = "examples/experimental/framework_project/providers/console_logger.orni";
  const source = 'import contracts.logger.{Logger};\nprovider ConsoleLogger for Logger { func log(message: string) -> unit { print unknown_value; } }';
  const result = run(["--check-module-buffers", module], root, `[files]\n${JSON.stringify(module)} = ${JSON.stringify(source)}\n`);
  assert.equal(result.status, 1, result.stderr);
  assert.match(result.stderr, /ORNI-E103/);
  assert.match(result.stderr, /unknown_value/);
  assert.equal(run(["check-module", module]).status, 0);
  const empty = run(["--check-module-buffers", module], root, "files = {}\n");
  assert.equal(empty.status, 0, empty.stderr);
});

test("large and deeply nested input fails with a diagnostic instead of trapping", () => {
  const inferenceChain = Array.from({ length: 70 }, (_, index) => `func f${index}() -> string { return ${index === 69 ? '"end"' : `f${index + 1}()`}; }`).join("\n");
  const directory = mkdtempSync(path.join(os.tmpdir(), "orni-limits-"));
  try {
    const sources = [" ".repeat(1048577), "print 1;".repeat(7000), inferenceChain, `print ${"(".repeat(100)}1${")".repeat(100)};`, `print ${Array(300).fill("1").join("+")};`];
    for (const [index, source] of sources.entries()) {
      const file = path.join(directory, `input-${index}.orni`);
      writeFileSync(file, source);
      const result = run(["check", file]);
      assert.equal(result.status, 1, result.stderr);
      assert.match(result.stderr, /error \[ORNI-/);
      assert.doesNotMatch(result.stderr, /RuntimeError|unreachable|call stack/i);
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
