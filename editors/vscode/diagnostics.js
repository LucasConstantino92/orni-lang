const vscode = require("vscode");
const { startCompiler } = require("./compiler-client");
const { isTestFile, projectFor, resolveModule } = require("./project-support");
const { diagnosticGuidance } = require("./diagnostic-guidance");
const fs = require("node:fs");
const path = require("node:path");
const TOML = require("@iarna/toml");
const { DependencyIndex } = require("./dependency-index");
const { findImports, findVariables, syntaxMask } = require("./language-support");

/** Owns compiler processes, snapshots, and workspace analysis for one activation. */
function createDiagnostics(extensionPath) {
  const diagnosticTimers = new Map();
  const diagnosticProcesses = new Map();
  const compilerDiagnosticUris = new Map();

  /** Resolves the application's manifest root or the current file directory. */
  function compilerProjectFor(document) {
    const project = projectFor(document.uri.fsPath);
    return { uri: vscode.Uri.file(project?.root ?? path.dirname(document.uri.fsPath)), project };
  }

  const compilerSnapshots = new Map();
  const publishedDiagnostics = new Map();
  const dependencyIndex = new DependencyIndex();
  const importRoots = new Map();
  const completedFingerprints = new Map();
  const pendingChecks = new Map();
  const dirtyWorkspaceFiles = new Set();
  const unsavedDocuments = new Set();
  const statistics = { compilerRuns: 0, cacheHits: 0, workspaceScans: 0 };
  let fullWorkspaceScan = true;
  let analysisQueue = Promise.resolve();
  let stopped = false;

  /** Refreshes import ownership from the latest buffer or saved source. */
  function indexFile(file, sourceRoot) {
    try {
      const buffer = vscode.workspace.textDocuments.find((item) => item.uri.fsPath === file && !item.isClosed && item.isDirty);
      // Preserve the compiler's resource boundaries during lightweight indexing too.
      const source = buffer ? buffer.getText() : fs.statSync(file).size <= 1048576 ? fs.readFileSync(file, "utf8") : "";
      const roots = importRoots.get(file) || new Set();
      roots.add(sourceRoot ?? projectFor(file)?.source ?? path.dirname(file));
      importRoots.set(file, roots);
      const dependencies = new Set();
      const imports = findImports(source);
      for (const root of roots) {
        for (const imported of imports) {
          try { dependencies.add(resolveModule(file, imported.segments, root)); }
          catch { /* The compiler reports ambiguous or invalid module paths. */ }
        }
      }
      dependencyIndex.set(file, source, dependencies);
      return dependencies;
    } catch {
      dependencyIndex.delete(file);
      return [];
    }
  }

  /** Reads only the requested import graph, not unrelated workspace files. */
  function analysisSnapshot(entry, folder, document) {
    const pending = new Set([entry]);
    for (const file of pending) {
      for (const dependency of indexFile(file, folder.project?.source ?? path.dirname(entry))) {
        if (pending.size < 256) pending.add(dependency);
      }
    }
    const snapshot = dependencyIndex.snapshot(entry);
    const manifest = folder.project ? fs.readFileSync(folder.project.manifestPath, "utf8") : "";
    const configured = vscode.workspace.getConfiguration("orni", document.uri).get("compilerPath", "");
    return { ...snapshot, fingerprint: JSON.stringify([snapshot.fingerprint, manifest, configured]) };
  }

  /** Starts a bundled or explicitly configured compiler process. */
  function compilerProcess(folder, args, document) {
    const configured = vscode.workspace.getConfiguration("orni", document.uri).get("compilerPath", "");
    return startCompiler(extensionPath, folder.uri.fsPath, args, configured);
  }

  /** Creates a visible diagnostic when the compiler itself could not run. */
  function compilerFailureDiagnostic(document, message) {
    const diagnostic = new vscode.Diagnostic(
      new vscode.Range(0, 0, 0, Math.max(1, document.lineAt(0).text.length)),
      message,
      vscode.DiagnosticSeverity.Warning,
    );
    diagnostic.source = "Orni extension";
    diagnostic.code = "orni.compilerUnavailable";
    return diagnostic;
  }

  /** Extracts one useful line from a failed compiler process. */
  function compilerFailureMessage(output) {
    const lines = output
      .replace(/\x1b\[[0-9;]*m/g, "")
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter(Boolean);
    const detail = lines.at(-1);
    return detail
      ? `The Orni compiler could not check this file: ${detail}`
      : "The Orni compiler could not check this file.";
  }

  /** Converts a compiler diagnostic into an editor marker. */
  function diagnosticFromCompiler(document, folder, output) {
    const pattern = /(?:Lexical|Syntax|Semantic|Runtime|Module) (error|warning) \[(ORNI-[A-Z]\d+)\](?: in '([^']+)')? at line (\d+), column (\d+) to line (\d+), column (\d+): ([^\r\n]+)/g;
    return Array.from(output.matchAll(pattern), (match) => {
      const sourcePath = match[3]
        ? path.resolve(folder.uri.fsPath, match[3])
        : document.uri.fsPath;
      const uri = vscode.Uri.file(sourcePath);
      let lines;
      try {
        const openDocument = vscode.workspace.textDocuments.find((candidate) => candidate.uri.fsPath === sourcePath);
        lines = openDocument
          ? openDocument.getText().split(/\r?\n/)
          : fs.readFileSync(sourcePath, "utf8").split(/\r?\n/);
      } catch {
        lines = document.getText().split(/\r?\n/);
      }
      const startLine = Math.min(Number(match[4]) - 1, Math.max(0, lines.length - 1));
      const endLine = Math.min(Number(match[6]) - 1, Math.max(0, lines.length - 1));
      const column = (line, number) => Array.from(line || "").slice(0, Math.max(0, number - 1)).join("").length;
      const startColumn = column(lines[startLine], Number(match[5]));
      const endColumn = column(lines[endLine], Number(match[7]));
      const diagnostic = new vscode.Diagnostic(
        new vscode.Range(
          startLine,
          startColumn,
          endLine,
          startLine === endLine ? Math.max(startColumn + 1, endColumn) : endColumn,
        ),
        match[8],
        match[1] === "warning"
          ? vscode.DiagnosticSeverity.Warning
          : vscode.DiagnosticSeverity.Error,
      );
      diagnostic.source = "Orni compiler";
      diagnostic.code = match[2] === "ORNI-T001" && /expects string<\d+>, but found string<\d+>/.test(match[8])
        ? "orni.stringCapacity"
        : match[2];
      const guide = diagnosticGuidance(diagnostic);
      diagnostic.relatedInformation = [new vscode.DiagnosticRelatedInformation(
        new vscode.Location(uri, diagnostic.range),
        `How to fix: ${guide.steps.join(" ")}`,
      )];
      return { uri, diagnostic };
    });
  }

  /** Adds lightweight unused-variable hints; semantic diagnostics come from the compiler. */
  function lintDiagnostics(document) {
    const source = document.getText();
    const mask = syntaxMask(source);
    return findVariables(source).flatMap((variable) => {
      if (variable.name.startsWith("_")) return [];
      const uses = mask.match(new RegExp(`\\b${variable.name}\\b`, "g")) || [];
      if (uses.length > 1) return [];
      const start = document.positionAt(variable.offset);
      const diagnostic = new vscode.Diagnostic(
        new vscode.Range(start, start.translate(0, variable.name.length)),
        `Variable '${variable.name}' is never used.`,
        vscode.DiagnosticSeverity.Warning,
      );
      diagnostic.source = "Orni lint";
      diagnostic.code = "orni.unusedVariable";
      return [diagnostic];
    });
  }

  /** Rebuilds Problems from all application checks, preserving shared-module ownership. */
  function publishDiagnostics(collection) {
    const grouped = new Map();
    const add = (uri, diagnostic) => {
      const key = uri.toString();
      const group = grouped.get(key) || { uri, diagnostics: [], seen: new Set() };
      const identity = [diagnostic.code, diagnostic.range.start.line, diagnostic.range.start.character,
        diagnostic.range.end.line, diagnostic.range.end.character, diagnostic.message].join(":");
      if (!group.seen.has(identity)) {
        group.seen.add(identity);
        group.diagnostics.push(diagnostic);
      }
      grouped.set(key, group);
    };
    for (const snapshot of compilerSnapshots.values()) {
      for (const { uri, diagnostic } of snapshot) add(uri, diagnostic);
    }
    for (const document of vscode.workspace.textDocuments) {
      if (document.languageId !== "orni") continue;
      for (const diagnostic of lintDiagnostics(document).filter((item) => item.code === "orni.unusedVariable")) {
        add(document.uri, diagnostic);
      }
    }
    for (const key of publishedDiagnostics.keys()) {
      if (!grouped.has(key)) {
        collection.delete(vscode.Uri.parse(key));
        publishedDiagnostics.delete(key);
      }
    }
    for (const [key, { uri, diagnostics, seen }] of grouped) {
      const fingerprint = [...seen].sort().join("\n");
      if (publishedDiagnostics.get(key) === fingerprint) continue;
      for (const diagnostic of diagnostics) {
        if (diagnostic.relatedInformation?.length) continue;
        diagnostic.relatedInformation = [new vscode.DiagnosticRelatedInformation(
          new vscode.Location(uri, diagnostic.range),
          `How to fix: ${diagnosticGuidance(diagnostic).steps.join(" ")}`,
        )];
      }
      collection.set(uri, diagnostics);
      publishedDiagnostics.set(key, fingerprint);
    }
  }

  /** Runs analysis serially and rejects results from superseded editor buffers. */
  async function updateDiagnostics(document, diagnostics, asModule = false, background = false, asTest = false) {
    if (stopped || document.languageId !== "orni" || document.isClosed) return;
    let folder;
    try {
      folder = compilerProjectFor(document);
    } catch (error) {
      const diagnostic = compilerFailureDiagnostic(document, error.message);
      diagnostic.code = "ORNI-E130";
      diagnostic.severity = vscode.DiagnosticSeverity.Error;
      compilerSnapshots.set(`${document.uri}:configuration`, [{ uri: document.uri, diagnostic }]);
      publishDiagnostics(diagnostics);
      return;
    }
    compilerSnapshots.delete(`${document.uri}:configuration`);
    const entry = asModule || asTest ? document.uri.fsPath : folder.project?.entry ?? document.uri.fsPath;
    const key = vscode.Uri.file(entry).toString() + (asModule ? ":module" : "");
    const snapshot = analysisSnapshot(entry, folder, document);
    if (completedFingerprints.get(key) === snapshot.fingerprint && !(asModule && !background)) {
      statistics.cacheHits++;
      publishDiagnostics(diagnostics);
      return;
    }
    if (pendingChecks.get(key)?.fingerprint === snapshot.fingerprint) return pendingChecks.get(key).promise;
    const version = document.version;
    const relatedBuffers = vscode.workspace.textDocuments.filter((buffer) => {
      if (buffer.languageId !== "orni" || buffer.isClosed) return false;
      return snapshot.files.has(buffer.uri.fsPath) && buffer.isDirty;
    });
    const versions = relatedBuffers.map((buffer) => [buffer, buffer.version]);
    const generation = (analysisGenerations.get(key) || 0) + 1;
    analysisGenerations.set(key, generation);
    diagnosticProcesses.get(key)?.kill();
    publishDiagnostics(diagnostics);
    const run = async () => {
      if (stopped || analysisGenerations.get(key) !== generation || document.version !== version) return;
      const relative = (file) => path.relative(folder.uri.fsPath, file).split(path.sep).join("/");
      const args = asModule
        ? ["--check-module-buffers", relative(entry)]
        : ["--check-buffers", relative(entry)];
      await new Promise((resolve) => {
        let child;
        try { child = compilerProcess(folder, args, document); }
        catch (error) {
          compilerSnapshots.set(key, [{ uri: document.uri, diagnostic: compilerFailureDiagnostic(document, error.message) }]);
          publishDiagnostics(diagnostics);
          resolve();
          return;
        }
        diagnosticProcesses.set(key, child);
        statistics.compilerRuns++;
        let output = "";
        let failure;
        const timeout = setTimeout(() => { failure = "Analysis exceeded the 15-second time limit."; child.kill(); }, 15000);
        const collect = (chunk) => {
          if (output.length + chunk.length > 1048576) { failure = "Compiler diagnostic output exceeded 1 MiB."; child.kill(); }
          else output += chunk;
        };
        child.stdout.on("data", collect);
        child.stderr.on("data", collect);
        child.stdin.on("error", () => {});
        child.on("error", (error) => { failure = error.message; });
        child.on("close", (code) => {
          clearTimeout(timeout);
          if (diagnosticProcesses.get(key) === child) diagnosticProcesses.delete(key);
          if (stopped || analysisGenerations.get(key) !== generation || versions.some(([buffer, version]) => buffer.isClosed || buffer.version !== version)) { resolve(); return; }
          const entries = diagnosticFromCompiler(document, folder, output);
          if (failure || (code !== 0 && entries.length === 0)) {
            entries.push({ uri: document.uri, diagnostic: compilerFailureDiagnostic(document, failure || compilerFailureMessage(output)) });
          }
          compilerSnapshots.set(key, entries);
          if (!failure && (code === 0 || entries.some(({ diagnostic }) => diagnostic.source === "Orni compiler"))) {
            completedFingerprints.set(key, snapshot.fingerprint);
          }
          if (!asModule) compilerDiagnosticUris.set(vscode.Uri.file(entry).toString(), new Set(entries.map(({ uri }) => uri)));
          publishDiagnostics(diagnostics);
          if (asModule && !background) {
            const message = entries.find(({ diagnostic }) => diagnostic.severity === vscode.DiagnosticSeverity.Error)?.diagnostic.message;
            if (message || failure || code !== 0) vscode.window.showErrorMessage(message || failure || compilerFailureMessage(output));
            else vscode.window.showInformationMessage(`${path.basename(entry)} is a valid reusable Orni module.`);
          }
          resolve();
        });
        child.stdin.end(TOML.stringify({ files: Object.fromEntries(relatedBuffers.map((buffer) => [relative(buffer.uri.fsPath), buffer.getText()])) }));
      });
    };
    const pending = analysisQueue.then(run);
    pendingChecks.set(key, { fingerprint: snapshot.fingerprint, promise: pending });
    analysisQueue = pending.catch(() => {});
    try { await pending; }
    finally { if (pendingChecks.get(key)?.promise === pending) pendingChecks.delete(key); }
  }

  const analysisGenerations = new Map();

  let workspaceScan;
  let workspaceScanAgain = false;
  let workspaceScanTimer;

  /** Checks unopened files and retains their Problems markers after editors close. */
  async function analyzeWorkspace(diagnostics, full = true) {
    if (stopped) return;
    if (full) fullWorkspaceScan = true;
    if (workspaceScan) { workspaceScanAgain = true; return workspaceScan; }
    workspaceScan = (async () => {
      do {
        workspaceScanAgain = false;
        let files;
        if (fullWorkspaceScan) {
          statistics.workspaceScans++;
          fullWorkspaceScan = false;
          files = await vscode.workspace.findFiles("**/*.orni", "**/{.git,target,node_modules,.vscode-test}/**");
          const existing = new Set(files.map((uri) => uri.fsPath));
          for (const file of dependencyIndex.nodes.keys()) if (!existing.has(file)) {
            dependencyIndex.delete(file);
            importRoots.delete(file);
          }
          for (const uri of files) indexFile(uri.fsPath);
        } else {
          files = [...dependencyIndex.affected(dirtyWorkspaceFiles)].filter((file) => fs.existsSync(file)).map((file) => vscode.Uri.file(file));
        }
        dirtyWorkspaceFiles.clear();
        let processed = 0;
        for (const uri of files) {
          if (stopped) break;
          if (++processed % 32 === 0) await new Promise((resolve) => setImmediate(resolve));
          try {
            const document = await vscode.workspace.openTextDocument(uri);
            let project;
            try { project = projectFor(uri.fsPath); }
            catch { /* updateDiagnostics publishes persistent project-setting errors. */ }
            const asTest = isTestFile(uri.fsPath, project);
            await updateDiagnostics(document, diagnostics, Boolean(project && project.entry !== uri.fsPath && !asTest), true, asTest);
          } catch (error) {
            console.error(`Could not analyze ${uri.fsPath}: ${error.message}`);
          }
        }
      } while (workspaceScanAgain && !stopped);
    })();
    try { await workspaceScan; } finally { workspaceScan = undefined; }
  }

  function scheduleWorkspaceAnalysis(diagnostics, uri) {
    if (stopped) return;
    if (!uri || path.basename(uri.fsPath) === "orni.toml") fullWorkspaceScan = true;
    else dirtyWorkspaceFiles.add(uri.fsPath);
    clearTimeout(workspaceScanTimer);
    workspaceScanTimer = setTimeout(() => {
      workspaceScanTimer = undefined;
      analyzeWorkspace(diagnostics, false).catch(console.error);
    }, 400);
  }

  /** Cancels affected pending checks immediately, before the debounce expires. */
  function invalidateFiles(files) {
    const affected = dependencyIndex.affected(files);
    for (const file of affected) {
      const origin = vscode.Uri.file(file).toString();
      for (const key of [origin, `${origin}:module`]) {
        analysisGenerations.set(key, (analysisGenerations.get(key) || 0) + 1);
        diagnosticProcesses.get(key)?.kill();
        pendingChecks.delete(key);
      }
    }
  }

  /** Invalidates saved changes and coalesces background checks for affected importers. */
  function fileChanged(uri, diagnostics, topologyChanged = false) {
    if (stopped) return;
    if (topologyChanged || path.basename(uri.fsPath) === "orni.toml") {
      for (const key of pendingChecks.keys()) {
        analysisGenerations.set(key, (analysisGenerations.get(key) || 0) + 1);
        diagnosticProcesses.get(key)?.kill();
      }
      pendingChecks.clear();
      if (path.basename(uri.fsPath) === "orni.toml") {
        completedFingerprints.clear();
        compilerSnapshots.clear();
        compilerDiagnosticUris.clear();
      } else if (!fs.existsSync(uri.fsPath)) {
        const origin = uri.toString();
        for (const key of [origin, `${origin}:module`, `${origin}:configuration`]) {
          completedFingerprints.delete(key);
          compilerSnapshots.delete(key);
        }
        compilerDiagnosticUris.delete(origin);
        for (const [key, entries] of compilerSnapshots) {
          compilerSnapshots.set(key, entries.filter((entry) => entry.uri.toString() !== origin));
        }
      }
      publishDiagnostics(diagnostics);
      scheduleWorkspaceAnalysis(diagnostics);
    } else {
      invalidateFiles([uri.fsPath]);
      scheduleWorkspaceAnalysis(diagnostics, uri);
    }
  }

  /** Drops compiler caches after configuration changes without touching source files. */
  function refresh(diagnostics) {
    completedFingerprints.clear();
    for (const document of vscode.workspace.textDocuments) scheduleDiagnostics(document, diagnostics);
    scheduleWorkspaceAnalysis(diagnostics);
  }

  /** Closing an unsaved buffer restores saved-source diagnostics in its importers. */
  function closeDocument(document, diagnostics) {
    if (document.languageId !== "orni") return;
    const key = document.uri.toString();
    clearTimeout(diagnosticTimers.get(key));
    diagnosticTimers.delete(key);
    // Editors may evict clean background documents automatically. Reopening
    // them on every close would create an endless analysis/eviction loop.
    if (!unsavedDocuments.delete(key)) return;
    invalidateFiles([document.uri.fsPath]);
    scheduleWorkspaceAnalysis(diagnostics, document.uri);
  }

  /** Checks one reusable module through the same bounded analysis queue. */
  async function checkModuleFromEditor(uri, diagnostics) {
    if (!uri) return;
    await updateDiagnostics(await vscode.workspace.openTextDocument(uri), diagnostics, true);
  }

  /** Debounces edits immediately, invalidating an older in-flight check. */
  function scheduleDiagnostics(document, diagnostics) {
    if (stopped || document.languageId !== "orni") return;
    const key = document.uri.toString();
    if (document.isDirty) unsavedDocuments.add(key);
    // Remember edits until close: revert-and-close can deliver a clean buffer
    // event before the close event, cancelling the pending debounce timer.
    clearTimeout(diagnosticTimers.get(key));
    invalidateFiles([document.uri.fsPath]);
    indexFile(document.uri.fsPath);
    try {
      const project = projectFor(document.uri.fsPath);
      const origin = vscode.Uri.file(project?.entry ?? document.uri.fsPath).toString();
      analysisGenerations.set(origin, (analysisGenerations.get(origin) || 0) + 1);
      diagnosticProcesses.get(origin)?.kill();
    } catch {
      // Manifest errors are published by the scheduled analysis.
    }
    diagnosticTimers.set(key, setTimeout(() => {
      diagnosticTimers.delete(key);
      let asTest = false;
      try { asTest = isTestFile(document.uri.fsPath, projectFor(document.uri.fsPath)); }
      catch { /* updateDiagnostics publishes project configuration errors. */ }
      updateDiagnostics(document, diagnostics, false, false, asTest).catch((error) => {
        if (!document.isClosed) diagnostics.set(document.uri, [compilerFailureDiagnostic(document, error.message)]);
      });
      scheduleWorkspaceAnalysis(diagnostics, document.uri);
    }, 300));
  }

  /** Deactivates the extension. Registered resources are disposed by VS Code. */
  function stop() {
    stopped = true;
    clearTimeout(workspaceScanTimer);
    for (const timer of diagnosticTimers.values()) clearTimeout(timer);
    for (const child of diagnosticProcesses.values()) child.kill();
    diagnosticTimers.clear();
    diagnosticProcesses.clear();
    compilerSnapshots.clear();
    publishedDiagnostics.clear();
    compilerDiagnosticUris.clear();
    completedFingerprints.clear();
    pendingChecks.clear();
    dependencyIndex.clear();
    importRoots.clear();
    dirtyWorkspaceFiles.clear();
    unsavedDocuments.clear();
    analysisGenerations.clear();
  }


  return { updateDiagnostics, analyzeWorkspace, scheduleWorkspaceAnalysis, scheduleDiagnostics, checkModuleFromEditor, compilerDiagnosticUris, fileChanged, refresh, closeDocument, stop,
    statistics: () => ({ ...statistics, pendingChecks: pendingChecks.size, runningChecks: diagnosticProcesses.size,
      workspaceScanning: Boolean(workspaceScan), scheduledChecks: diagnosticTimers.size + Number(Boolean(workspaceScanTimer)),
    }),
  };
}
module.exports = { createDiagnostics };
