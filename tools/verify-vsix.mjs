import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { pipeline } from "node:stream/promises";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const require = createRequire(path.join(root, "editors/vscode/package.json"));
const yauzl = require("yauzl");
const packagePath = path.resolve(process.argv[2]);
const directory = fs.mkdtempSync(path.join(os.tmpdir(), "orni-vsix-"));
try {
  await new Promise((resolve, reject) => {
    yauzl.open(packagePath, { lazyEntries: true }, (error, zip) => {
      if (error) return reject(error);
      zip.on("error", reject);
      zip.on("end", resolve);
      zip.on("entry", async (entry) => {
        try {
          assert(!entry.fileName.split("/").includes(".."));
          const destination = path.join(directory, entry.fileName);
          if (entry.fileName.endsWith("/")) fs.mkdirSync(destination, { recursive: true });
          else {
            fs.mkdirSync(path.dirname(destination), { recursive: true });
            const stream = await new Promise((resolve, reject) => zip.openReadStream(entry, (error, stream) => error ? reject(error) : resolve(stream)));
            await pipeline(stream, fs.createWriteStream(destination));
          }
          zip.readEntry();
        } catch (error) { zip.close(); reject(error); }
      });
      zip.readEntry();
    });
  });
  const extension = path.join(directory, "extension");
  const packageRequire = createRequire(path.join(extension, "package.json"));
  assert(packageRequire.resolve("@iarna/toml").startsWith(extension));
  for (const file of ["extension.js", "language-support.js", "compiler-client.js", "project-support.js", "diagnostic-guidance.js", "diagnostic-catalog.js", "dependency-index.js", "diagnostics.js", "diagnostic-ui.js", "code-actions.js", "module-actions.js", "editor-symbols.js", "editor-navigation.js"]) {
    assert.equal(fs.readFileSync(path.join(extension, file), "utf8"), fs.readFileSync(path.join(root, "editors/vscode", file), "utf8"));
  }
  assert.equal(fs.readFileSync(path.join(extension, "compiler/wasi-runner.mjs"), "utf8"), fs.readFileSync(path.join(root, "tools/wasi-runner.mjs"), "utf8"));
  for (const [source, expected] of [['print "ready";', 0], ["print missing;", 1]]) {
    const result = spawnSync(process.execPath, ["--no-warnings", path.join(extension, "compiler/wasi-runner.mjs"), path.join(extension, "compiler/orni.wasm"), "--check-stdin"], {
      cwd: directory, input: source, encoding: "utf8", timeout: 15000,
    });
    assert.equal(result.status, expected, result.stderr);
  }
  console.log("Verified VSIX contents, runtime dependency, and bundled compiler exit statuses outside the checkout.");
} finally { fs.rmSync(directory, { recursive: true, force: true }); }
