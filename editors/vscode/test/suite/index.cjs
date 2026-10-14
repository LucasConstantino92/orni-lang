const assert = require("node:assert/strict");
const vscode = require("vscode");
const path = require("node:path");
const fs = require("node:fs");

async function eventually(predicate, label) {
  const deadline = Date.now() + 20000;
  while (Date.now() < deadline) {
    const result = await predicate();
    if (result) return result;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(`Timed out: ${typeof label === "function" ? label() : label}\n${JSON.stringify(vscode.languages.getDiagnostics().map(([uri, items]) => [uri.fsPath, items.map(({ message }) => message)]))}`);
}

async function actionsFor(document) {
  const diagnostic = await eventually(() => vscode.languages.getDiagnostics(document.uri).find(({ message }) => message.includes("has no provider binding")), "missing binding diagnostic");
  return eventually(async () => {
    try {
      const actions = await vscode.commands.executeCommand("vscode.executeCodeActionProvider", document.uri, diagnostic.range, vscode.CodeActionKind.QuickFix.value);
      return actions?.length ? actions : undefined;
    } catch (error) {
      if (error.name === "Canceled" || error.message === "Canceled") return undefined;
      throw error;
    }
  }, "registered Quick Fix provider");
}

exports.run = async function () {
  console.log("START: Orni Extension Host integration tests");
  try {
  const extension = vscode.extensions.getExtension("orni-lang.orni-language");
  assert(extension);
  await extension.activate();
  assert(extension.packageJSON.contributes.keybindings.some(({ key, command }) => key === "ctrl+[Period]" && command === "orni.quickFix"));
  const workspace = vscode.workspace.workspaceFolders[0].uri.fsPath;
  await vscode.commands.executeCommand("orni.analyzeWorkspace");
  const unopened = vscode.Uri.file(path.join(workspace, "errors/intentional.orni"));
  assert(vscode.languages.getDiagnostics(unopened).some(({ message }) => message.includes("missing_value")), JSON.stringify(vscode.languages.getDiagnostics().map(([uri, items]) => [uri.fsPath, items.map(({ message }) => message)])));
  assert(!vscode.window.visibleTextEditors.some(({ document }) => document.uri.toString() === unopened.toString()));
  console.log("PASS: workspace errors appear in Problems before opening a file");
  const initialStatistics = extension.exports.analysisStatistics();
  await vscode.commands.executeCommand("orni.analyzeWorkspace");
  assert.equal(extension.exports.analysisStatistics().compilerRuns, initialStatistics.compilerRuns, "Repeating a workspace check must reuse unchanged graph results.");
  console.log("PASS: repeated analysis of a 65-file project starts no additional compiler processes");
  const intentional = await vscode.workspace.openTextDocument(unopened);
  const intentionalDiagnostic = vscode.languages.getDiagnostics(unopened).find(({ message }) => message.includes("missing_value"));
  const explanationActions = await vscode.commands.executeCommand("vscode.executeCodeActionProvider", unopened, intentionalDiagnostic.range, vscode.CodeActionKind.QuickFix.value);
  const explanation = explanationActions.find(({ command }) => command?.command === "orni.explainDiagnostic");
  assert(explanation, "Every diagnostic must offer manual correction guidance.");
  assert.match(explanation.title, /Declare or import the missing name/);
  assert(explanation.command.arguments[0].message.includes("missing_value"));
  assert.equal(intentionalDiagnostic.code, "ORNI-E103");
  assert(intentionalDiagnostic.relatedInformation.some(({ message }) => message.includes("How to fix:") && message.includes("let before")));
  const hovers = await vscode.commands.executeCommand("vscode.executeHoverProvider", unopened, intentionalDiagnostic.range.start);
  assert(hovers.some(({ contents }) => contents.some((content) => content.value?.includes("How to fix") && content.value.replaceAll("&nbsp;", " ").includes("let before"))));
  assert.equal(intentional.isDirty, false, "Reading correction guidance must not change source files.");
  console.log("PASS: manual guidance is available in Quick Fix, Problems, and error hover");
  const standalone = await vscode.workspace.openTextDocument(path.join(workspace, "errors/pending_binding_fix.orni"));
  await vscode.window.showTextDocument(standalone);
  const standaloneActions = await actionsFor(standalone);
  assert(standaloneActions.some(({ title }) => title === "Bind 'Clock' to 'SystemClock'"), JSON.stringify(standaloneActions.map(({ title }) => title)));
  console.log("PASS: the exact standalone missing-binding example offers its binding Quick Fix");
  for (const [project, expected] of [["application", "Bind 'Clock' to 'SystemClock'"], ["generator", "Create 'ClockProvider' for 'Clock'"]]) {
    const root = path.join(workspace, project);
    const moduleDocument = await vscode.workspace.openTextDocument(path.join(root, "providers/audit.orni"));
    await vscode.window.showTextDocument(moduleDocument);
    const actions = await actionsFor(moduleDocument);
    const action = actions.find(({ title }) => title === expected);
    assert(action, JSON.stringify(actions.map(({ title }) => title)));
    if (project === "application") {
      const clockOffset = moduleDocument.getText().indexOf("clock: Clock") + "clock: ".length;
      const position = moduleDocument.positionAt(clockOffset);
      vscode.window.activeTextEditor.selection = new vscode.Selection(position, position);
      let finished = false;
      const command = vscode.commands.executeCommand("orni.quickFix").finally(() => { finished = true; });
      const deadline = Date.now() + 15000;
      while (!finished && Date.now() < deadline) {
        await new Promise((resolve) => setTimeout(resolve, 200));
        await vscode.commands.executeCommand("workbench.action.acceptSelectedQuickOpenItem");
      }
      assert(finished, "The Orni Quick Fix command must complete.");
      await command;
    } else {
      assert(await vscode.workspace.applyEdit(action.edit));
    }
    assert(await vscode.workspace.saveAll(false));
    const main = await vscode.workspace.openTextDocument(path.join(root, "main.orni"));
    assert.match(main.getText(), /bind Clock to (?:SystemClock|ClockProvider);/);
    assert.doesNotMatch(moduleDocument.getText(), /bind Clock/);
    await vscode.window.showTextDocument(main);
    await vscode.commands.executeCommand("orni.validateApplication");
    await eventually(() => !vscode.languages.getDiagnostics(moduleDocument.uri).some(({ severity }) => severity === vscode.DiagnosticSeverity.Error), `binding fix removed errors in ${project}`);
    if (project === "generator") assert(fs.existsSync(path.join(root, "providers/clock_provider.orni")));
    const offset = moduleDocument.getText().indexOf("contracts.clock") + 2;
    const locations = await vscode.commands.executeCommand("vscode.executeDefinitionProvider", moduleDocument.uri, moduleDocument.positionAt(offset));
    assert.equal(locations[0].uri.fsPath, path.join(root, "contracts/clock.orni"));
    console.log(`PASS: real editor diagnostics, Quick Fix, WorkspaceEdit and navigation (${project})`);
  }
  const single = path.join(workspace, "single.orni");
  fs.writeFileSync(single, "print 1;\n");
  const document = await vscode.workspace.openTextDocument(single);
  await vscode.window.showTextDocument(document);
  const edit = new vscode.WorkspaceEdit();
  edit.replace(document.uri, new vscode.Range(0, 0, 0, 8), "print missing;");
  await vscode.workspace.applyEdit(edit);
  await eventually(() => vscode.languages.getDiagnostics(document.uri).some(({ message }) => message.includes("undefined variable 'missing'")), "live unsaved error");
  const fix = new vscode.WorkspaceEdit();
  fix.replace(document.uri, new vscode.Range(0, 0, 0, 14), "print 2;");
  await vscode.workspace.applyEdit(fix);
  await eventually(() => !vscode.languages.getDiagnostics(document.uri).some(({ severity }) => severity === vscode.DiagnosticSeverity.Error), "stale error removed");
  console.log("PASS: unsaved diagnostics and stale marker cleanup outside compiler checkout");
  const applicationRoot = path.join(workspace, "application");
  const clock = await vscode.workspace.openTextDocument(path.join(applicationRoot, "providers/clock.orni"));
  const audit = await vscode.workspace.openTextDocument(path.join(applicationRoot, "providers/audit.orni"));
  const main = await vscode.workspace.openTextDocument(path.join(applicationRoot, "main.orni"));
  const changeMain = new vscode.WorkspaceEdit();
  changeMain.replace(main.uri, new vscode.Range(0, 0, main.lineCount, 0), main.getText().replaceAll("SystemClock", "UpdatedClock"));
  assert(await vscode.workspace.applyEdit(changeMain));
  await eventually(() => vscode.languages.getDiagnostics(main.uri).some(({ severity }) => severity === vscode.DiagnosticSeverity.Error), "unsaved import rename reports a missing provider");
  const changeClock = new vscode.WorkspaceEdit();
  changeClock.replace(clock.uri, new vscode.Range(0, 0, clock.lineCount, 0), clock.getText().replace("SystemClock", "UpdatedClock"));
  assert(await vscode.workspace.applyEdit(changeClock));
  await vscode.window.showTextDocument(audit);
  await eventually(() => !vscode.languages.getDiagnostics(main.uri).some(({ severity }) => severity === vscode.DiagnosticSeverity.Error), "multiple unsaved buffers analyzed together");
  assert.match(fs.readFileSync(clock.uri.fsPath, "utf8"), /SystemClock/);
  assert.match(fs.readFileSync(main.uri.fsPath, "utf8"), /SystemClock/);
  console.log("PASS: multiple unsaved modules are analyzed as one application");

  // Let debounced checks from the preceding scenarios finish before measuring.
  await new Promise((resolve) => setTimeout(resolve, 1000));
  await eventually(() => {
    const stats = extension.exports.analysisStatistics();
    return !stats.pendingChecks && !stats.runningChecks && !stats.workspaceScanning && !stats.scheduledChecks;
  }, () => `analysis queue and background workspace scan settle: ${JSON.stringify(extension.exports.analysisStatistics())}`);
  const beforeIncremental = extension.exports.analysisStatistics();
  const orphan = await vscode.workspace.openTextDocument(path.join(workspace, "scale/library63.orni"));
  const originalOrphan = fs.readFileSync(orphan.uri.fsPath, "utf8");
  const orphanEdit = new vscode.WorkspaceEdit();
  orphanEdit.replace(orphan.uri, new vscode.Range(0, 0, orphan.lineCount, 0), originalOrphan.replace("return 63", "return unknown_scale_value"));
  assert(await vscode.workspace.applyEdit(orphanEdit));
  await eventually(() => vscode.languages.getDiagnostics(orphan.uri).some(({ code }) => code === "ORNI-E103"), "an unused module is checked from its unsaved buffer");
  await new Promise((resolve) => setTimeout(resolve, 600));
  const afterIncremental = extension.exports.analysisStatistics();
  assert.equal(afterIncremental.workspaceScans, beforeIncremental.workspaceScans, "An ordinary edit must not rediscover the entire workspace.");
  assert.equal(afterIncremental.compilerRuns - beforeIncremental.compilerRuns, 1, "Only the changed, unimported module should need a compiler process.");
  assert.equal(fs.readFileSync(orphan.uri.fsPath, "utf8"), originalOrphan, "Background module analysis must never save the user's buffer.");
  console.log("PASS: editing one module in a 65-file project checks exactly one file, without a workspace scan or save");

  await vscode.window.showTextDocument(orphan);
  await vscode.commands.executeCommand("workbench.action.revertAndCloseActiveEditor");
  await eventually(() => !vscode.languages.getDiagnostics(orphan.uri).some(({ code }) => code === "ORNI-E103"), "discarding an unsaved buffer restores saved-source diagnostics");
  console.log("PASS: discard-and-close removes unsaved errors without changing the saved module");

  const createdUri = vscode.Uri.file(path.join(workspace, "created.orni"));
  await vscode.workspace.fs.writeFile(createdUri, Buffer.from("print created_missing;\n"));
  await eventually(() => vscode.languages.getDiagnostics(createdUri).some(({ code }) => code === "ORNI-E103"), "new files enter workspace Problems");
  await vscode.workspace.fs.delete(createdUri);
  await eventually(() => vscode.languages.getDiagnostics(createdUri).length === 0, "deleted files leave workspace Problems");
  console.log("PASS: creating and deleting files invalidates workspace diagnostics correctly");

  const manifestUri = vscode.Uri.file(path.join(workspace, "scale/orni.toml"));
  const originalManifest = fs.readFileSync(manifestUri.fsPath, "utf8");
  const scaleEntry = vscode.Uri.file(path.join(workspace, "scale/main.orni"));
  await vscode.workspace.fs.writeFile(manifestUri, Buffer.from('[project]\nsource = "../outside"\n'));
  await eventually(() => vscode.languages.getDiagnostics(scaleEntry).some(({ code }) => code === "ORNI-E130"), "invalid manifests publish stable project guidance");
  await vscode.workspace.fs.writeFile(manifestUri, Buffer.from(originalManifest));
  await eventually(() => !vscode.languages.getDiagnostics(scaleEntry).some(({ severity }) => severity === vscode.DiagnosticSeverity.Error), "restoring the manifest clears project errors");
  console.log("PASS: manifest errors persist in Problems and clear after correcting the configuration");
  } catch (error) {
    console.error("ORNI HOST TEST FAILURE:", error.stack || error);
    throw error;
  }
};
