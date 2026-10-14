const { runTests } = require("@vscode/test-electron");
const fs = require("node:fs");
const path = require("node:path");
const os = require("node:os");

async function main() {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "orni-host-"));
  const workspace = path.join(directory, "workspace");
  try {
    fs.mkdirSync(workspace);
    fs.mkdirSync(path.join(workspace, "errors"));
    fs.writeFileSync(path.join(workspace, "errors/intentional.orni"), '// Intentional semantic error: this example tests workspace diagnostics.\nprint missing_value;\n');
    fs.copyFileSync(path.resolve(__dirname, "../../..", "examples/experimental/errors/pending_binding_fix.orni"), path.join(workspace, "errors/pending_binding_fix.orni"));
    const scaleRoot = path.join(workspace, "scale");
    fs.mkdirSync(scaleRoot);
    fs.writeFileSync(path.join(scaleRoot, "orni.toml"), '[project]\nname = "scale"\nentry = "main.orni"\nsource = "."\n');
    fs.writeFileSync(path.join(scaleRoot, "main.orni"), 'import library0.{value0};\nprint value0();\n');
    for (let index = 0; index < 64; index++) {
      fs.writeFileSync(path.join(scaleRoot, `library${index}.orni`), `func value${index}() -> int { return ${index}; }\n`);
    }
    for (const name of ["application", "generator"]) {
      const root = path.join(workspace, name);
      fs.mkdirSync(path.join(root, "contracts"), { recursive: true });
      fs.mkdirSync(path.join(root, "providers"));
      fs.writeFileSync(path.join(root, "orni.toml"), `[project]\nname = "${name}"\nentry = "main.orni"\nsource = "."\n`);
      fs.writeFileSync(path.join(root, "contracts/clock.orni"), "contract Clock {\n    func now() -> int;\n}\n");
      fs.writeFileSync(path.join(root, "contracts/audit.orni"), "contract Audit {\n    func record() -> unit;\n}\n");
      fs.writeFileSync(path.join(root, "providers/audit.orni"), 'import contracts.clock.{Clock};\nimport contracts.audit.{Audit};\nprovider TimedAudit(clock: Clock) for Audit {\n    func record() -> unit { print clock.now(); }\n}\n');
      fs.writeFileSync(path.join(root, "main.orni"), 'import contracts.audit.{Audit};\nimport providers.audit.{TimedAudit};\nbind Audit to TimedAudit;\nfunc start(audit: Audit) -> unit { audit.record(); }\nstart();\n');
      if (name === "application") {
        fs.writeFileSync(path.join(root, "providers/clock.orni"), 'import contracts.clock.{Clock};\nprovider SystemClock for Clock {\n    func now() -> int { return 42; }\n}\n');
        fs.appendFileSync(path.join(root, "main.orni"), 'import contracts.clock.{Clock};\nimport providers.clock.{SystemClock};\n');
      }
    }
    await runTests({
      ...(process.env.ORNI_EDITOR_EXECUTABLE
        ? { vscodeExecutablePath: process.env.ORNI_EDITOR_EXECUTABLE }
        : { version: "1.141.2" }),
      extensionDevelopmentPath: path.resolve(__dirname, ".."),
      extensionTestsPath: path.join(__dirname, "suite", "index.cjs"),
      launchArgs: [workspace, "--disable-extensions", "--disable-workspace-trust", "--disable-gpu", "--no-sandbox", "--skip-welcome", "--skip-release-notes", "--user-data-dir", path.join(directory, "user-data")],
    });
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
}
main().catch((error) => { console.error(error); process.exitCode = 1; });
