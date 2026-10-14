const vscode = require("vscode");

const { diagnosticGuidance } = require("./diagnostic-guidance");

const { findImports, findStringCapacityAnnotation, sortedImportStatements } = require("./language-support");

const { workspacePublicMembers, autoImportEdit, compositionDocumentsFor: compositionsFor, missingBindingActions } = require("./module-actions");

/** Creates actions with access to application ownership snapshots. */
function createCodeActions(compilerDiagnosticUris) {
  const compositionDocumentsFor = (document) => compositionsFor(document, compilerDiagnosticUris);
  /** Creates context-aware fixes shown by the editor's Ctrl+. menu. */
  async function codeActionsFor(document, context) {
    const actions = [];
    const source = document.getText();

    for (const diagnostic of context.diagnostics) {
      if (diagnostic.code === "ORNI-W004") {
        const line = diagnostic.range.start.line;
        const start = new vscode.Position(line, 0);
        const end = line + 1 < document.lineCount
          ? new vscode.Position(line + 1, 0)
          : document.lineAt(line).range.end;
        const remove = new vscode.CodeAction(
          "Remove unused contract binding",
          vscode.CodeActionKind.QuickFix,
        );
        remove.edit = new vscode.WorkspaceEdit();
        remove.edit.delete(document.uri, new vscode.Range(start, end));
        remove.diagnostics = [diagnostic];
        remove.isPreferred = true;
        actions.push(remove);
      }

      const missingBinding = diagnostic.message.match(
        /contract '([A-Za-z_][A-Za-z0-9_.]*)' has no provider binding/,
      )?.[1];
      if (missingBinding) {
        const compositions = await compositionDocumentsFor(document);
        for (const composition of compositions) {
          const fixes = await missingBindingActions(composition, diagnostic, missingBinding);
          for (const action of fixes) {
            if (compositions.length > 1) {
              action.title += ` in '${vscode.workspace.asRelativePath(composition.uri)}'`;
              action.isPreferred = false;
            }
            actions.push(action);
          }
        }
      }

      if (diagnostic.code === "ORNI-E106" || diagnostic.code === "orni.stringCapacity") {
        const capacities = diagnostic.message.match(
          /(?:expects string<(\d+)>, but found string<(\d+)>|requires (\d+) UTF-8 bytes, but string<(\d+)>)/,
        );
        if (!capacities) continue;
        const maximum = capacities[1] || capacities[4];
        const actual = capacities[2] || capacities[3];
        const diagnosticOffset = document.offsetAt(diagnostic.range.start);
        const annotation = findStringCapacityAnnotation(source, diagnosticOffset, maximum);
        if (!annotation) continue;
        const annotationRange = new vscode.Range(
          document.positionAt(annotation.offset),
          document.positionAt(annotation.offset + annotation.length),
        );

        const increase = new vscode.CodeAction(
          `Increase string capacity to ${actual} bytes`,
          vscode.CodeActionKind.QuickFix,
        );
        increase.edit = new vscode.WorkspaceEdit();
        increase.edit.replace(document.uri, annotationRange, `string<${actual}>`);
        increase.diagnostics = [diagnostic];
        increase.isPreferred = true;
        actions.push(increase);

        const infer = new vscode.CodeAction(
          "Use automatic string capacity inference",
          vscode.CodeActionKind.QuickFix,
        );
        infer.edit = new vscode.WorkspaceEdit();
        infer.edit.replace(document.uri, annotationRange, "string");
        infer.diagnostics = [diagnostic];
        actions.push(infer);
      }

      if (diagnostic.code === "orni.unusedVariable") {
        const name = document.getText(diagnostic.range);
        if (name.startsWith("_")) continue;
        const prefix = new vscode.CodeAction(
          `Mark '${name}' as intentionally unused`,
          vscode.CodeActionKind.QuickFix,
        );
        prefix.edit = new vscode.WorkspaceEdit();
        prefix.edit.replace(document.uri, diagnostic.range, `_${name}`);
        prefix.diagnostics = [diagnostic];
        prefix.isPreferred = true;
        actions.push(prefix);
      }

      if (diagnostic.code === "ORNI-W001" || diagnostic.code === "ORNI-W002") {
        const moduleImport = findImports(source).find((candidate) => {
          return document.positionAt(candidate.statementOffset).line === diagnostic.range.start.line;
        });
        if (!moduleImport) continue;
        const selectedMember = moduleImport.members.find(
          ({ offset }) => document.positionAt(offset).line === diagnostic.range.start.line
            && diagnostic.range.contains(document.positionAt(offset)),
        );
        if (diagnostic.code === "ORNI-W002" && selectedMember && moduleImport.members.length > 1) {
          const memberIndex = moduleImport.members.indexOf(selectedMember);
          const nextMember = moduleImport.members[memberIndex + 1];
          const previousMember = moduleImport.members[memberIndex - 1];
          const removalStart = nextMember
            ? selectedMember.offset
            : previousMember.offset + previousMember.length;
          const removalEnd = nextMember
            ? nextMember.offset
            : selectedMember.offset + selectedMember.length;
          const removeMember = new vscode.CodeAction(
            `Remove unused imported member '${selectedMember.name}'`,
            vscode.CodeActionKind.QuickFix,
          );
          removeMember.edit = new vscode.WorkspaceEdit();
          removeMember.edit.delete(
            document.uri,
            new vscode.Range(
              document.positionAt(removalStart),
              document.positionAt(removalEnd),
            ),
          );
          removeMember.diagnostics = [diagnostic];
          removeMember.isPreferred = true;
          actions.push(removeMember);
          continue;
        }
        let endOffset = moduleImport.statementOffset + moduleImport.statementLength;
        if (source.slice(endOffset, endOffset + 2) === "\r\n") endOffset += 2;
        else if (source[endOffset] === "\n") endOffset += 1;
        const remove = new vscode.CodeAction(
          `Remove unused import '${selectedMember?.name || moduleImport.namespace}'`,
          vscode.CodeActionKind.QuickFix,
        );
        remove.edit = new vscode.WorkspaceEdit();
        remove.edit.delete(
          document.uri,
          new vscode.Range(
            document.positionAt(moduleImport.statementOffset),
            document.positionAt(endOffset),
          ),
        );
        remove.diagnostics = [diagnostic];
        remove.isPreferred = true;
        actions.push(remove);
      }

      const missingName = diagnostic.message.match(/undefined (?:variable|function) '([A-Za-z_][A-Za-z0-9_]*)'/)?.[1];
      if (missingName) {
        const candidates = (await workspacePublicMembers(document)).filter(
          ({ name }) => name === missingName,
        );
        for (const candidate of candidates) {
          const importEdit = autoImportEdit(document, candidate);
          if (!importEdit) continue;
          const addImport = new vscode.CodeAction(
            `Import '${candidate.name}' from '${candidate.modulePath}'`,
            vscode.CodeActionKind.QuickFix,
          );
          addImport.edit = new vscode.WorkspaceEdit();
          addImport.edit.set(document.uri, [importEdit]);
          addImport.diagnostics = [diagnostic];
          addImport.isPreferred = candidates.length === 1;
          actions.push(addImport);
        }
      }
    }

    for (const diagnostic of context.diagnostics) {
      const guide = diagnosticGuidance(diagnostic);
      const explain = new vscode.CodeAction(`Explain how to fix: ${guide.title}`, vscode.CodeActionKind.QuickFix);
      explain.diagnostics = [diagnostic];
      explain.command = {
        command: "orni.explainDiagnostic",
        title: explain.title,
        arguments: [{ message: diagnostic.message, code: typeof diagnostic.code === "object" ? diagnostic.code.value : diagnostic.code }],
      };
      actions.push(explain);
    }

    const imports = findImports(source);
    if (imports.length > 1) {
      const sorted = sortedImportStatements(source);
      const current = imports.map((moduleImport) => source.slice(
        moduleImport.statementOffset,
        moduleImport.statementOffset + moduleImport.statementLength,
      ));
      if (current.some((statement, index) => statement !== sorted[index])) {
        const organize = new vscode.CodeAction(
          "Organize Orni imports",
          vscode.CodeActionKind.SourceOrganizeImports,
        );
        organize.edit = new vscode.WorkspaceEdit();
        organize.edit.set(
          document.uri,
          imports.map((moduleImport, index) => vscode.TextEdit.replace(
            new vscode.Range(
              document.positionAt(moduleImport.statementOffset),
              document.positionAt(moduleImport.statementOffset + moduleImport.statementLength),
            ),
            sorted[index],
          )),
        );
        actions.push(organize);
      }
    }

    const checkModule = new vscode.CodeAction(
      "Check this file as a reusable Orni module",
      vscode.CodeActionKind.Source,
    );
    checkModule.command = {
      command: "orni.checkModule",
      title: checkModule.title,
      arguments: [document.uri],
    };
    actions.push(checkModule);

    return actions;
  }


  return codeActionsFor;
}
module.exports = { createCodeActions };
