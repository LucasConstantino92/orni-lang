const vscode = require("vscode");
const { projectFor } = require("./project-support");
const { createDiagnostics } = require("./diagnostics");
const { createCodeActions } = require("./code-actions");
const { createDiagnosticUI } = require("./diagnostic-ui");
const { hoverFor, signatureHelpFor, inlayHintsFor } = require("./editor-symbols");
const { definitionFor, moduleCompletionsFor } = require("./editor-navigation");

let analysis;
let developmentTerminal;
let developmentRoot;

/** Starts an Orni development server in a named terminal. */
function launchDevelopmentServer(root) {
  developmentRoot = root;
  developmentTerminal = vscode.window.createTerminal({ name: "Orni Server", cwd: root });
  developmentTerminal.show(true);
  developmentTerminal.sendText("orni dev");
  return vscode.window.showInformationMessage("Starting the Orni development server.");
}

/** Finds the current project and starts its native development server. */
function startDevelopmentServer() {
  const current = vscode.window.activeTextEditor?.document;
  const project = current && current.languageId === "orni" ? projectFor(current.uri.fsPath) : undefined;
  const workspace = vscode.workspace.workspaceFolders?.[0];
  const root = project?.root || workspace?.uri.fsPath;
  if (!root) {
    return vscode.window.showErrorMessage("Open an Orni project before starting the development server.");
  }
  if (developmentTerminal) {
    developmentTerminal.show(true);
    return vscode.window.showInformationMessage("The Orni development server terminal is already open.");
  }
  return launchDevelopmentServer(root);
}

/** Stops the current development server by closing its terminal. */
function stopDevelopmentServer() {
  if (!developmentTerminal) {
    return vscode.window.showInformationMessage("No Orni development server is running.");
  }
  developmentTerminal.dispose();
  developmentTerminal = undefined;
  return vscode.window.showInformationMessage("Stopped the Orni development server.");
}

/** Restarts the most recently started development server. */
function restartDevelopmentServer() {
  if (developmentTerminal) developmentTerminal.dispose();
  developmentTerminal = undefined;
  if (developmentRoot) return launchDevelopmentServer(developmentRoot);
  return startDevelopmentServer();
}

/** Activates semantic navigation and documentation for Orni files. */
function activate(context) {
  analysis = createDiagnostics(context.extensionPath);
  const { updateDiagnostics, analyzeWorkspace, scheduleWorkspaceAnalysis, scheduleDiagnostics, checkModuleFromEditor, compilerDiagnosticUris, fileChanged, refresh, closeDocument } = analysis;
  const codeActionsFor = createCodeActions(compilerDiagnosticUris);
  const { showQuickFix, explainDiagnostic, diagnosticHover } = createDiagnosticUI(updateDiagnostics, codeActionsFor);
  const selector = { language: "orni", scheme: "file" };
  const diagnostics = vscode.languages.createDiagnosticCollection("orni");
  const watcher = vscode.workspace.createFileSystemWatcher("**/{*.orni,orni.toml}");

  context.subscriptions.push(
    diagnostics,
    vscode.commands.registerCommand("orni.quickFix", () => showQuickFix(diagnostics)),
    vscode.commands.registerCommand("orni.explainDiagnostic", (diagnostic) => {
      if (diagnostic) return explainDiagnostic(diagnostic);
      const editor = vscode.window.activeTextEditor;
      const current = editor && (diagnostics.get(editor.document.uri) || []);
      const selected = current && (current.find(({ range }) => range.contains(editor.selection.active)) || current[0]);
      if (selected) return explainDiagnostic(selected);
      return vscode.window.showInformationMessage("Place the cursor on an Orni diagnostic to read its correction steps.");
    }),
    vscode.commands.registerCommand("orni.analyzeWorkspace", () => analyzeWorkspace(diagnostics)),
    vscode.commands.registerCommand("orni.startDevelopmentServer", startDevelopmentServer),
    vscode.commands.registerCommand("orni.stopDevelopmentServer", stopDevelopmentServer),
    vscode.commands.registerCommand("orni.restartDevelopmentServer", restartDevelopmentServer),
    watcher,
    watcher.onDidCreate((uri) => fileChanged(uri, diagnostics, true)),
    watcher.onDidChange((uri) => fileChanged(uri, diagnostics)),
    watcher.onDidDelete((uri) => fileChanged(uri, diagnostics, true)),
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (event.affectsConfiguration("orni")) refresh(diagnostics);
    }),
    vscode.languages.registerHoverProvider(selector, {
      provideHover: hoverFor,
    }),
    vscode.languages.registerHoverProvider(selector, {
      provideHover: (document, position) => diagnosticHover(document, position, diagnostics),
    }),
    vscode.languages.registerDefinitionProvider(selector, {
      provideDefinition: definitionFor,
    }),
    vscode.languages.registerCodeActionsProvider(
      selector,
      { provideCodeActions: (_document, _range, context) => codeActionsFor(_document, context) },
      { providedCodeActionKinds: [vscode.CodeActionKind.QuickFix, vscode.CodeActionKind.Source] },
    ),
    vscode.commands.registerCommand("orni.checkModule", (uri) =>
      checkModuleFromEditor(
        uri || vscode.window.activeTextEditor?.document.uri,
        diagnostics,
      ),
    ),
    vscode.commands.registerCommand("orni.validateApplication", async () => {
      const current = vscode.window.activeTextEditor?.document;
      if (!current || current.languageId !== "orni") return;
      if (!(await vscode.workspace.saveAll(false))) return;
      const project = projectFor(current.uri.fsPath);
      const entry = project ? await vscode.workspace.openTextDocument(project.entry) : current;
      await updateDiagnostics(entry, diagnostics);
    }),
    vscode.languages.registerSignatureHelpProvider(
      selector,
      { provideSignatureHelp: signatureHelpFor },
      "(",
      ",",
    ),
    vscode.languages.registerInlayHintsProvider(selector, {
      provideInlayHints: inlayHintsFor,
    }),
    vscode.languages.registerCompletionItemProvider(
      selector,
      { provideCompletionItems: moduleCompletionsFor },
      ".",
    ),
    vscode.workspace.onDidSaveTextDocument((document) =>
      scheduleDiagnostics(document, diagnostics),
    ),
    vscode.workspace.onDidOpenTextDocument((document) => {
      if (vscode.window.visibleTextEditors.some((editor) => editor.document === document)) scheduleDiagnostics(document, diagnostics);
    }),
    vscode.window.onDidChangeActiveTextEditor((editor) => {
      if (editor) scheduleDiagnostics(editor.document, diagnostics);
    }),
    vscode.workspace.onDidChangeTextDocument(({ document, contentChanges }) => {
      if (contentChanges.length) scheduleDiagnostics(document, diagnostics);
    }),
    vscode.workspace.onDidCloseTextDocument((document) => closeDocument(document, diagnostics)),
    vscode.window.onDidCloseTerminal((terminal) => {
      if (terminal === developmentTerminal) developmentTerminal = undefined;
    }),
  );

  for (const document of vscode.workspace.textDocuments) {
    scheduleDiagnostics(document, diagnostics);
  }
  scheduleWorkspaceAnalysis(diagnostics);
  return { analysisStatistics: () => analysis?.statistics() };
}


/** Stops all pending checks; registered editor resources are disposed by VS Code. */
function deactivate() { analysis?.stop(); analysis = undefined; }

module.exports = { activate, deactivate };
