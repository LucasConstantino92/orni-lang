const vscode = require("vscode");

const { resolveModule } = require("./project-support");

const path = require("node:path");

const { findClosestDeclaration, findContracts, findFunctionParameters, findFunctions, findImports, findModels, findProviders, findStores, findVariables } = require("./language-support");

const { workspacePublicMembers, autoImportEdit } = require("./module-actions");

const { symbolAt, backendDefinitionFor } = require("./editor-symbols");

/** Resolves a symbol to its declaration or built-in documentation page. */
async function definitionFor(document, position) {
  const symbol = symbolAt(document, position);
  if (!symbol) return undefined;

  const source = document.getText();
  const usageOffset = document.offsetAt(position);
  const symbolOffset = document.offsetAt(symbol.range.start);
  const backendDefinition = await backendDefinitionFor(
    storeOperationAt(source, symbolOffset, symbol.name) ||
      responseOperationAt(source, symbolOffset, symbol.name) ||
      apiOperationAt(source, symbolOffset, symbol.name) ||
      symbol.name,
  );
  if (backendDefinition) return backendDefinition;

  const moduleImport = findImports(source).find(
    (candidate) =>
      usageOffset >= candidate.offset &&
      usageOffset <= candidate.offset + candidate.length,
  );
  if (moduleImport) {
    const modulePath = resolveModule(document.uri.fsPath, moduleImport.segments);
    return new vscode.Location(vscode.Uri.file(modulePath), new vscode.Position(0, 0));
  }
  const selectedImport = findImports(source).find((candidate) =>
    candidate.members.some(
      ({ offset, length }) => usageOffset >= offset && usageOffset <= offset + length,
    ),
  );
  const selectedName = selectedImport?.members.find(
    ({ offset, length }) => usageOffset >= offset && usageOffset <= offset + length,
  )?.name;
  if (selectedImport && selectedName) {
    const location = await importedMemberLocation(document, selectedImport, selectedName);
    if (location) return location;
  }
  const qualifiedRange = document.getWordRangeAtPosition(
    position,
    /[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)+/,
  );
  if (qualifiedRange) {
    const [namespace, member] = document.getText(qualifiedRange).split(".");
    const imported = findImports(source).find(
      (candidate) => candidate.namespace === namespace,
    );
    if (imported && member) {
      const modulePath = resolveModule(document.uri.fsPath, imported.segments);
      try {
        const moduleDocument = await vscode.workspace.openTextDocument(modulePath);
        const moduleSource = moduleDocument.getText();
        const declaration =
          findFunctions(moduleSource).find(({ name }) => name === member) ||
          findVariables(moduleSource).find(({ name }) => name === member) ||
          findContracts(moduleSource).find(({ name }) => name === member) ||
          findProviders(moduleSource).find(({ name }) => name === member);
        if (declaration && !member.startsWith("_")) {
          return new vscode.Location(
            moduleDocument.uri,
            moduleDocument.positionAt(declaration.offset),
          );
        }
      } catch {
        return undefined;
      }
    }
  }
  const declaration =
    findClosestDeclaration(findFunctions(source), symbol.name, usageOffset) ||
    findClosestDeclaration(findVariables(source), symbol.name, usageOffset) ||
    findClosestDeclaration(findModels(source), symbol.name, usageOffset) ||
    findClosestDeclaration(findStores(source), symbol.name, usageOffset) ||
    findFunctionParameters(source).find(
      (parameter) =>
        parameter.name === symbol.name &&
        usageOffset >= parameter.offset &&
        usageOffset <= parameter.scopeEnd,
    ) ||
    findContracts(source).find(({ name }) => name === symbol.name) ||
    findProviders(source).find(({ name }) => name === symbol.name);

  if (declaration) {
    return new vscode.Location(document.uri, document.positionAt(declaration.offset));
  }
  const directImport = findImports(source).find((candidate) =>
    candidate.members.some(({ name }) => name === symbol.name),
  );
  return directImport
    ? importedMemberLocation(document, directImport, symbol.name)
    : undefined;
}

/** Returns the native implementation key for a store operation under the cursor. */
function storeOperationAt(source, usageOffset, symbolName) {
  if (!new Set(["create", "list", "find", "replace", "patch", "delete"]).has(symbolName)) {
    return undefined;
  }
  const before = source.slice(Math.max(0, usageOffset - 160), usageOffset);
  return /[A-Za-z_][A-Za-z0-9_]*\s*\.\s*$/.test(before)
    ? `store.${symbolName}`
    : undefined;
}

/** Returns the native implementation key for a response helper under the cursor. */
function responseOperationAt(source, usageOffset, symbolName) {
  if (!new Set([
    "ok", "created", "not_found", "bad_request", "unauthorized", "forbidden",
    "conflict", "internal_error", "redirect", "no_content", "header",
  ]).has(symbolName)) {
    return undefined;
  }
  const before = source.slice(Math.max(0, usageOffset - 160), usageOffset);
  return /response\s*\.\s*$/.test(before)
    ? `response.${symbolName}`
    : undefined;
}

/** Returns the native implementation key for an API test helper under the cursor. */
function apiOperationAt(source, usageOffset, symbolName) {
  if (!new Set(["status", "body", "contains"]).has(symbolName)) return undefined;
  const before = source.slice(Math.max(0, usageOffset - 160), usageOffset);
  return /api\s*\.\s*$/.test(before) ? `api.${symbolName}` : undefined;
}

/** Resolves one public member inside the module named by an import. */
async function importedMemberLocation(importingDocument, moduleImport, memberName) {
  const modulePath = resolveModule(importingDocument.uri.fsPath, moduleImport.segments);
  try {
    const moduleDocument = await vscode.workspace.openTextDocument(modulePath);
    const declaration =
      findFunctions(moduleDocument.getText()).find(({ name }) => name === memberName) ||
      findVariables(moduleDocument.getText()).find(({ name }) => name === memberName) ||
      findContracts(moduleDocument.getText()).find(({ name }) => name === memberName) ||
      findProviders(moduleDocument.getText()).find(({ name }) => name === memberName);
    return declaration && !memberName.startsWith("_")
      ? new vscode.Location(moduleDocument.uri, moduleDocument.positionAt(declaration.offset))
      : undefined;
  } catch {
    return undefined;
  }
}

/** Completes public members after an imported module namespace. */
async function moduleCompletionsFor(document, position) {
  const prefix = document.getText(
    new vscode.Range(new vscode.Position(position.line, 0), position),
  );
  const access = prefix.match(/([A-Za-z_][A-Za-z0-9_]*)\.$/);
  if (!access) {
    const source = document.getText();
    const selectedMembers = findImports(source).flatMap((moduleImport) =>
      moduleImport.members.map(({ name }) => {
        const item = new vscode.CompletionItem(name, vscode.CompletionItemKind.Reference);
        item.detail = `Imported from ${moduleImport.path}`;
        return item;
      }),
    );
    const visibleNames = new Set([
      ...findFunctions(source).map(({ name }) => name),
      ...findVariables(source).map(({ name }) => name),
      ...selectedMembers.map(({ label }) => label),
    ]);
    const autoImports = [];
    for (const candidate of await workspacePublicMembers(document)) {
      if (visibleNames.has(candidate.name)) continue;
      const edit = autoImportEdit(document, candidate);
      if (!edit) continue;
      const item = new vscode.CompletionItem(
        candidate.name,
        candidate.kind === "function"
          ? vscode.CompletionItemKind.Function
          : candidate.kind === "contract"
            ? vscode.CompletionItemKind.Interface
            : candidate.kind === "provider"
              ? vscode.CompletionItemKind.Class
              : vscode.CompletionItemKind.Variable,
      );
      item.detail = `Auto import from ${candidate.modulePath}`;
      item.documentation = candidate.documentation || undefined;
      item.additionalTextEdits = [edit];
      item.sortText = `z-${candidate.name}`;
      autoImports.push(item);
    }
    return [...selectedMembers, ...autoImports];
  }
  const imported = findImports(document.getText()).find(
    (candidate) => !candidate.members.length && candidate.namespace === access[1],
  );
  if (!imported) return undefined;

  const modulePath = resolveModule(document.uri.fsPath, imported.segments);
  try {
    const moduleDocument = await vscode.workspace.openTextDocument(modulePath);
    const source = moduleDocument.getText();
    const functions = findFunctions(source)
      .filter(({ name }) => !name.startsWith("_"))
      .map((declaration) => {
        const item = new vscode.CompletionItem(
          declaration.name,
          vscode.CompletionItemKind.Function,
        );
        item.detail = `func ${declaration.name}(${declaration.parameters.join(", ")}) -> ${declaration.returnType}`;
        item.documentation = declaration.documentation || undefined;
        return item;
      });
    const variables = findVariables(source)
      .filter(({ name }) => !name.startsWith("_"))
      .map((declaration) => {
        const item = new vscode.CompletionItem(
          declaration.name,
          vscode.CompletionItemKind.Variable,
        );
        item.detail = declaration.typeName || declaration.inferredType || "inferred value";
        return item;
      });
    const contracts = findContracts(source)
      .filter(({ name }) => !name.startsWith("_"))
      .map((declaration) => {
        const item = new vscode.CompletionItem(
          declaration.name,
          vscode.CompletionItemKind.Interface,
        );
        item.detail = `contract ${declaration.name}`;
        return item;
      });
    const providers = findProviders(source)
      .filter(({ name }) => !name.startsWith("_"))
      .map((declaration) => {
        const item = new vscode.CompletionItem(
          declaration.name,
          vscode.CompletionItemKind.Class,
        );
        const dependencies = declaration.dependencies.length === 0
          ? ""
          : `(${declaration.dependencies.join(", ")})`;
        item.detail = `provider ${declaration.name}${dependencies} for ${declaration.contract}`;
        return item;
      });
    return [...functions, ...variables, ...contracts, ...providers];
  } catch {
    return undefined;
  }
}


module.exports = { definitionFor, moduleCompletionsFor };
