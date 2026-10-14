const vscode = require("vscode");

const { projectFor, resolveModule } = require("./project-support");

const fs = require("node:fs");
const path = require("node:path");

const { findBindings, findContracts, findFunctions, findImports, findVariables, findProviders, importInsertionOffset, providerSource, snakeCaseTypeName, topLevelDeclarations } = require("./language-support");

/** Discovers public declarations that can be imported from nearby modules. */
async function workspacePublicMembers(document) {
  const searchRoot = projectFor(document.uri.fsPath)?.source ?? path.dirname(document.uri.fsPath);
  const moduleUris = await vscode.workspace.findFiles(
    new vscode.RelativePattern(searchRoot, "**/*.orni"),
    "**/{.git,target,node_modules}/**",
  );
  const currentDirectory = projectFor(document.uri.fsPath)?.source ?? path.dirname(document.uri.fsPath);
  const members = [];
  for (const uri of moduleUris) {
    if (uri.fsPath === document.uri.fsPath) continue;
    const relativePath = path.relative(currentDirectory, uri.fsPath);
    if (relativePath.startsWith("..") || path.isAbsolute(relativePath)) continue;
    const modulePath = relativePath
      .replace(/\.orni$/i, "")
      .split(path.sep)
      .join(".");
    try {
      const moduleDocument = await vscode.workspace.openTextDocument(uri);
      const source = moduleDocument.getText();
      if (findBindings(source).length) continue;
      if (projectFor(uri.fsPath)?.root !== projectFor(document.uri.fsPath)?.root) continue;
      for (const declaration of topLevelDeclarations(source, findFunctions(source)).filter(({ name }) => !name.startsWith("_"))) {
        members.push({ ...declaration, modulePath, uri, kind: "function" });
      }
      for (const declaration of topLevelDeclarations(source, findVariables(source)).filter(({ name }) => !name.startsWith("_"))) {
        members.push({ ...declaration, modulePath, uri, kind: "variable" });
      }
      for (const declaration of topLevelDeclarations(source, findContracts(source)).filter(({ name }) => !name.startsWith("_"))) {
        members.push({ ...declaration, modulePath, uri, kind: "contract" });
      }
      for (const declaration of topLevelDeclarations(source, findProviders(source)).filter(({ name }) => !name.startsWith("_"))) {
        members.push({ ...declaration, modulePath, uri, kind: "provider" });
      }
    } catch {
      // Files that disappear during discovery are ignored until the next request.
    }
  }
  return members;
}

/** Builds the edit that adds one selective import to a document. */
function autoImportEdit(document, candidate) {
  const source = document.getText();
  const existing = findImports(source).find(
    ({ path: importedPath }) => importedPath === candidate.modulePath,
  );
  if (existing) {
    if (!existing.members.length) return undefined;
    if (existing.members.some(({ name }) => name === candidate.name)) return undefined;
    const closingBrace = source.indexOf(
      "}",
      existing.members.at(-1).offset + existing.members.at(-1).length,
    );
    if (closingBrace < 0) return undefined;
    return vscode.TextEdit.insert(document.positionAt(closingBrace), `, ${candidate.name}`);
  }
  const offset = importInsertionOffset(source);
  const suffix = offset === 0 ? "\n\n" : "\n";
  return vscode.TextEdit.insert(
    document.positionAt(offset),
    `import ${candidate.modulePath}.{${candidate.name}};${suffix}`,
  );
}

/** Appends one binding while preserving a readable entry-file layout. */
function bindingEdit(document, contractName, providerName) {
  const source = document.getText();
  const separator = source.endsWith("\n") ? "\n" : "\n\n";
  return vscode.TextEdit.insert(
    document.positionAt(source.length),
    `${separator}bind ${contractName} to ${providerName};\n`,
  );
}

/** Finds every application that owns a module diagnostic without guessing between roots. */
async function compositionDocumentsFor(document, compilerDiagnosticUris) {
  const candidates = new Map();
  const project = projectFor(document.uri.fsPath);
  if (project && fs.existsSync(project.entry)) {
    const entry = await vscode.workspace.openTextDocument(project.entry);
    candidates.set(entry.uri.toString(), entry);
  }
  const moduleKey = document.uri.toString();
  for (const [entryKey, diagnosticUris] of compilerDiagnosticUris) {
    if (![...diagnosticUris].some((uri) => uri.toString() === moduleKey)) continue;
    try {
      const entry = await vscode.workspace.openTextDocument(vscode.Uri.parse(entryKey));
      candidates.set(entryKey, entry);
    } catch {
      // A removed application cannot be offered as a correction target.
    }
  }
  return candidates.size ? [...candidates.values()] : [document];
}

/** Returns the declaration identity used by the compiler's root module namespace. */
function declarationIdentity(document, name, composition = document) {
  const project = projectFor(composition.uri.fsPath);
  const modulePath = path.relative(project?.source ?? path.dirname(composition.uri.fsPath), document.uri.fsPath)
    .replace(/\.orni$/i, "").split(path.sep).join(".");
  const declaration = findContracts(document.getText()).find((item) => item.name === name);
  if (declaration) return composition.uri.fsPath === document.uri.fsPath ? name : `${modulePath}.${name}`;
  const [namespace, member] = name.split(".");
  const imported = findImports(document.getText()).find((item) =>
    member ? item.namespace === namespace : item.members.some((item) => item.name === name));
  if (!imported) return name;
  const file = resolveModule(document.uri.fsPath, imported.segments);
  const relative = path.relative(project?.source ?? path.dirname(composition.uri.fsPath), file)
    .replace(/\.orni$/i, "").split(path.sep).join(".");
  return `${relative}.${member || name}`;
}

/** Chooses a visible reference or adds an import without duplicating a declaration. */
function referenceFor(document, candidate, edits) {
  if (candidate.local) return candidate.name;
  const imported = findImports(document.getText()).find((item) => item.path === candidate.modulePath);
  if (imported?.members.some(({ name }) => name === candidate.name)) return candidate.name;
  if (imported?.namespace) return `${imported.namespace}.${candidate.name}`;
  const edit = autoImportEdit(document, candidate);
  if (edit) edits.push(edit);
  return candidate.name;
}

/** Coalesces insertion edits sharing a position so WorkspaceEdit remains deterministic. */
function combineInsertions(edits) {
  const combined = new Map();
  for (const edit of edits) {
    const key = `${edit.range.start.line}:${edit.range.start.character}`;
    const previous = combined.get(key);
    if (previous) previous.newText += edit.newText;
    else combined.set(key, vscode.TextEdit.insert(edit.range.start, edit.newText));
  }
  return [...combined.values()];
}

/** Offers binding fixes and provider creation for the exact missing contract identity. */
async function missingBindingActions(document, diagnostic, missingContract) {
  const source = document.getText();
  const contractName = missingContract.split(".").at(-1);
  if (findBindings(source).some(({ contract }) => declarationIdentity(document, contract) === missingContract)) return [];
  const publicMembers = await workspacePublicMembers(document);
  const contracts = [
    ...findContracts(source).map((contract) => ({ ...contract, local: true, uri: document.uri })),
    ...publicMembers.filter(({ kind }) => kind === "contract"),
  ].filter((contract) => {
    if (contract.name !== contractName) return false;
    return contract.local ? declarationIdentity(document, contract.name) === missingContract
      : `${contract.modulePath}.${contract.name}` === missingContract;
  });
  // A local single-file dependency has no module prefix.
  const contract = contracts[0];
  if (!contract) return [];
  const providers = [];
  for (const candidate of [
    ...findProviders(source).map((provider) => ({ ...provider, local: true, uri: document.uri })),
    ...publicMembers.filter(({ kind }) => kind === "provider"),
  ]) {
    const providerDocument = candidate.local ? document : await vscode.workspace.openTextDocument(candidate.uri);
    if (declarationIdentity(providerDocument, candidate.contract, document) === missingContract) providers.push(candidate);
  }
  const actions = [];
  for (const provider of providers) {
    const edits = [];
    const contractReference = referenceFor(document, contract, edits);
    const providerReference = referenceFor(document, provider, edits);
    edits.push(bindingEdit(document, contractReference, providerReference));
    const action = new vscode.CodeAction(`Bind '${contractName}' to '${provider.name}'`, vscode.CodeActionKind.QuickFix);
    action.edit = new vscode.WorkspaceEdit();
    action.edit.set(document.uri, combineInsertions(edits));
    action.diagnostics = [diagnostic];
    action.isPreferred = providers.length === 1;
    actions.push(action);
  }
  if (providers.length) return actions;

  const providerName = `${contractName}Provider`;
  if ([...findProviders(source), ...publicMembers].some(({ name }) => name === providerName)) return actions;
  const action = new vscode.CodeAction(`Create '${providerName}' for '${contractName}'`, vscode.CodeActionKind.QuickFix);
  action.edit = new vscode.WorkspaceEdit();
  if (contract.local) {
    action.edit.insert(document.uri, document.positionAt(source.length),
      `\n\n${providerSource(contract, providerName)}\nbind ${contractName} to ${providerName};\n`);
  } else {
    const root = projectFor(document.uri.fsPath)?.source ?? path.dirname(document.uri.fsPath);
    const providersDirectory = path.join(root, "providers");
    const directory = fs.existsSync(providersDirectory) ? providersDirectory : root;
    const providerUri = vscode.Uri.file(path.join(directory, `${snakeCaseTypeName(contractName)}_provider.orni`));
    if (fs.existsSync(providerUri.fsPath)) return actions;
    const providerModulePath = path.relative(root, providerUri.fsPath).replace(/\.orni$/i, "").split(path.sep).join(".");
    const contractDocument = await vscode.workspace.openTextDocument(contract.uri);
    const inheritedImports = findImports(contractDocument.getText()).map((item) => {
      const importedFile = resolveModule(contract.uri.fsPath, item.segments);
      const modulePath = path.relative(root, importedFile).replace(/\.orni$/i, "").split(path.sep).join(".");
      return `import ${modulePath}${item.members.length ? `.{${item.members.map(({ name }) => name).join(", ")}}` : item.alias ? ` as ${item.alias}` : ""};\n`;
    }).join("");
    action.edit.createFile(providerUri, { ignoreIfExists: false });
    action.edit.insert(providerUri, new vscode.Position(0, 0),
      inheritedImports + providerSource(contract, providerName, contract.modulePath));
    const edits = [];
    const contractReference = referenceFor(document, contract, edits);
    const providerReference = referenceFor(document, { name: providerName, modulePath: providerModulePath }, edits);
    edits.push(bindingEdit(document, contractReference, providerReference));
    action.edit.set(document.uri, combineInsertions(edits));
  }
  action.diagnostics = [diagnostic];
  actions.push(action);
  return actions;
}

/** Creates context-aware fixes shown by the editor's Ctrl+. menu. */

module.exports = { workspacePublicMembers, autoImportEdit, compositionDocumentsFor, missingBindingActions };
