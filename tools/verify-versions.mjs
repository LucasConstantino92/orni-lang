import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const require = createRequire(path.join(root, "editors/vscode/package.json"));
const TOML = require("@iarna/toml");
const cargo = TOML.parse(fs.readFileSync(path.join(root, "Cargo.toml"), "utf8"));
const lock = TOML.parse(fs.readFileSync(path.join(root, "Cargo.lock"), "utf8"));
const extension = JSON.parse(fs.readFileSync(path.join(root, "editors/vscode/package.json"), "utf8"));
assert.equal(extension.version, cargo.package.version, "Cargo and VSIX versions must match.");
assert.equal(lock.package.find(({ name }) => name === cargo.package.name).version, extension.version, "Cargo.lock must match the package version.");
const tag = process.argv[2];
if (tag) assert.equal(tag, `v${extension.version}`, "Release tag must match Cargo and VSIX versions.");
console.log(`Verified Orni version ${extension.version}${tag ? ` (${tag})` : ""}.`);
