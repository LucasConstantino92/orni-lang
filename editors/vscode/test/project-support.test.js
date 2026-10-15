const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const os = require("node:os");
const test = require("node:test");
const { isTestFile, projectFor, resolveModule } = require("../project-support");

test("manifest-free nested imports can use the owning entry's source root", () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "orni-entry-root-"));
  try {
    fs.mkdirSync(path.join(directory, "nested"));
    const helper = path.join(directory, "helper.orni");
    fs.writeFileSync(helper, "func get() -> int { return 1; }");
    assert.equal(resolveModule(path.join(directory, "nested/service.orni"), ["helper"], directory), helper);
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});

test("nested modules use the manifest source directory and detect ambiguous imports", () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "orni-project-"));
  try {
    fs.mkdirSync(path.join(directory, "src/providers"), { recursive: true });
    fs.mkdirSync(path.join(directory, "src/contracts"));
    fs.writeFileSync(path.join(directory, "orni.toml"), '[project]\nname = "example"\nsource = "src"\nentry = "src/main.orni"\n');
    fs.writeFileSync(path.join(directory, "src/contracts/clock.orni"), "contract Clock {}");
    const module = path.join(directory, "src/providers/clock.orni");
    assert.equal(projectFor(module).source, path.join(directory, "src"));
    assert.equal(resolveModule(module, ["contracts", "clock"]), path.join(directory, "src/contracts/clock.orni"));
    fs.mkdirSync(path.join(directory, "src/providers/contracts"));
    fs.writeFileSync(path.join(directory, "src/providers/contracts/clock.orni"), "contract Clock {}");
    assert.throws(() => resolveModule(module, ["contracts", "clock"]), /ambiguous/);
    fs.writeFileSync(path.join(directory, "orni.toml"), '[project]\nsource = "../escape"\n');
    assert.throws(() => projectFor(module), /inside/);
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});

test("recognizes executable test files without treating reusable modules as tests", () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "orni-tests-"));
  try {
    fs.mkdirSync(path.join(directory, "src"));
    fs.mkdirSync(path.join(directory, "tests"));
    fs.writeFileSync(path.join(directory, "orni.toml"), '[project]\nname = "example"\nsource = "src"\nentry = "src/main.orni"\n');
    const project = projectFor(path.join(directory, "tests/health.orni"));
    assert.equal(isTestFile(path.join(directory, "tests/health.orni"), project), true);
    assert.equal(isTestFile(path.join(directory, "src/services/health.orni"), project), false);
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});
