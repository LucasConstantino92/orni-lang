const vscode = require("vscode");

const { diagnosticGuidance } = require("./diagnostic-guidance");


/** Connects diagnostic help to the editor without owning analysis state. */
function createDiagnosticUI(updateDiagnostics, codeActionsFor) {
  /** Shows concrete fixes through an Orni command independent of editor keybinding defaults. */
  async function showQuickFix(diagnostics) {
    const editor = vscode.window.activeTextEditor;
    if (!editor || editor.document.languageId !== "orni") return;
    await updateDiagnostics(editor.document, diagnostics);
    const all = diagnostics.get(editor.document.uri) || [];
    const onLine = all.filter((diagnostic) => diagnostic.range.start.line <= editor.selection.active.line && diagnostic.range.end.line >= editor.selection.active.line);
    const selected = onLine.length ? onLine : all;
    const actions = (await codeActionsFor(editor.document, { diagnostics: selected }))
      .filter((action) => action.kind?.value === vscode.CodeActionKind.QuickFix.value);
    if (!actions.length) {
      vscode.window.showInformationMessage("No Orni diagnostics were found in this file.");
      return;
    }
    const chosen = await vscode.window.showQuickPick(actions.map((action) => ({
      label: action.title,
      description: action.command?.command === "orni.explainDiagnostic" ? "Manual guidance" : "Automatic edit",
      action,
    })), { placeHolder: "Select an automatic edit or an explanation of how to fix the diagnostic" });
    if (!chosen) return;
    if (chosen.action.edit) await vscode.workspace.applyEdit(chosen.action.edit);
    if (chosen.action.command) await vscode.commands.executeCommand(chosen.action.command.command, ...(chosen.action.command.arguments || []));
  }

  /** Displays manual correction steps without applying an edit. */
  function explainDiagnostic(diagnostic) {
    const guide = diagnosticGuidance(diagnostic);
    const detail = [diagnostic.message, "", ...guide.steps.map((step, index) => `${index + 1}. ${step}`), ...(guide.example ? ["", "Example:", guide.example] : [])].join("\n");
    return vscode.window.showInformationMessage(`How to fix: ${guide.title}`, { modal: true, detail }, "Got it");
  }

  /** Shows the same guidance alongside compiler messages when hovering an error. */
  function diagnosticHover(document, position, diagnostics) {
    const relevant = (diagnostics.get(document.uri) || []).filter(({ range }) => range.contains(position));
    if (!relevant.length) return undefined;
    const contents = relevant.map((diagnostic) => {
      const guide = diagnosticGuidance(diagnostic);
      const markdown = new vscode.MarkdownString();
      markdown.appendText(diagnostic.message);
      markdown.appendMarkdown("\n\n**How to fix**\n\n");
      for (const step of guide.steps) {
        markdown.appendMarkdown("- ");
        markdown.appendText(step);
        markdown.appendMarkdown("\n");
      }
      if (guide.example) markdown.appendCodeblock(guide.example, "orni");
      return markdown;
    });
    return new vscode.Hover(contents);
  }


  return { showQuickFix, explainDiagnostic, diagnosticHover };
}
module.exports = { createDiagnosticUI };
