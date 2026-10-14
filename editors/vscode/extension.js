const vscode = require("vscode");
const { spawn } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");
const {
  findClosestDeclaration,
  findBoundedStringViolations,
  findBoundedVariableViolations,
  findFunctions,
  findImports,
  findStringCapacityAnnotation,
  findUnusedImports,
  findUnusedPrivateFunctions,
  findVariables,
  importInsertionOffset,
  identifierPattern,
  sortedImportStatements,
} = require("./language-support");

const builtins = {
  print: {
    signature: "print value;",
    summary: "Evaluates a value, converts it to text, and appends it to the program output followed by a newline.",
    details: [
      "`print` is a built-in statement implemented by the Orni interpreter.",
      "It accepts numbers, strings, booleans, and the `unit` value.",
      "The statement itself does not produce a reusable value.",
    ],
  },
};

const backendSymbols = {
  print: {
    path: ["src", "runtime", "interpreter.rs"],
    needle: "Statement::Print {",
  },
  func: {
    path: ["src", "runtime", "interpreter.rs"],
    needle: "Statement::Function {",
  },
};

const keywordDocumentation = {
  let: "Declares a variable in the current lexical scope.",
  func: "Declares a named function with parameters and a block body.",
  return: "Stops the current function and yields an optional value.",
  if: "Executes a branch when its condition evaluates to a boolean `true`.",
  else: "Provides the branch executed when an `if` condition is `false`.",
  true: "The boolean true value.",
  false: "The boolean false value.",
  import: "Loads public functions and values through a module namespace.",
  as: "Assigns a local namespace alias to an imported module.",
};

const primitiveTypes = {
  string: ["Dynamically sized UTF-8 text.", 'let name = "Orni"; // inferred as string<4>'],
  int: ["A signed 64-bit integer with checked arithmetic.", "let workers: int = 8;"],
  double: ["A 64-bit floating-point number.", "let ratio: double = 0.75;"],
  bool: ["A logical value that is either true or false.", "let enabled: bool = true;"],
  char: ["One Unicode scalar value.", "let initial: char = 'O';"],
  unit: ["The absence of a reusable value.", "func log() -> unit { print \"done\"; }"],
};

const diagnosticTimers = new Map();
const diagnosticProcesses = new Map();
const compilerDiagnosticUris = new Map();

/** Returns the identifier and range under the cursor. */
function symbolAt(document, position) {
  const range = document.getWordRangeAtPosition(position, identifierPattern);
  if (!range) return undefined;
  return { name: document.getText(range), range };
}

/** Locates the Rust implementation behind an Orni built-in or keyword. */
async function backendDefinitionFor(name) {
  const target = backendSymbols[name];
  if (!target) return undefined;

  for (const folder of vscode.workspace.workspaceFolders || []) {
    const uri = vscode.Uri.joinPath(folder.uri, ...target.path);
    try {
      const document = await vscode.workspace.openTextDocument(uri);
      const offset = document.getText().indexOf(target.needle);
      if (offset >= 0) {
        return new vscode.Location(uri, document.positionAt(offset));
      }
    } catch {
      // The current workspace may use the Orni extension without compiler sources.
    }
  }

  return undefined;
}

/** Builds rich hover content for a symbol. */
async function hoverFor(document, position) {
  const symbol = symbolAt(document, position);
  if (!symbol) return undefined;

  const source = document.getText();
  const usageOffset = document.offsetAt(position);
  const moduleImport = findImports(source).find(
    (candidate) =>
      usageOffset >= candidate.offset &&
      usageOffset <= candidate.offset + candidate.length,
  );
  if (moduleImport) {
    const modulePath = path.join(
      path.dirname(document.uri.fsPath),
      ...moduleImport.segments,
    ) + ".orni";
    try {
      const moduleDocument = await vscode.workspace.openTextDocument(modulePath);
      const moduleSource = moduleDocument.getText();
      const publicFunctions = findFunctions(moduleSource).filter(
        ({ name }) => !name.startsWith("_"),
      );
      const publicVariables = findVariables(moduleSource).filter(
        ({ name }) => !name.startsWith("_"),
      );
      const markdown = new vscode.MarkdownString();
      markdown.appendCodeblock(`module ${moduleImport.path}`, "orni");
      markdown.appendMarkdown(`\nDefined in \`${modulePath}\`\n\n**Public members**\n`);
      for (const declaration of publicFunctions) {
        markdown.appendMarkdown(
          `\n- \`func ${declaration.name}(${declaration.parameters.join(", ")}) -> ${declaration.returnType}\`${declaration.documentation ? ` — ${declaration.documentation}` : ""}`,
        );
      }
      for (const declaration of publicVariables) {
        markdown.appendMarkdown(
          `\n- \`let ${declaration.name}: ${declaration.typeName || declaration.inferredType || "inferred"}\``,
        );
      }
      if (!publicFunctions.length && !publicVariables.length) {
        markdown.appendMarkdown("\n- No public members");
      }
      return new vscode.Hover(markdown);
    } catch {
      return new vscode.Hover(
        new vscode.MarkdownString(`Module \`${moduleImport.path}\` could not be opened.`),
      );
    }
  }

  const builtin = builtins[symbol.name];
  if (builtin) {
    const markdown = new vscode.MarkdownString();
    markdown.appendCodeblock(builtin.signature, "orni");
    markdown.appendMarkdown(`\n${builtin.summary}`);
    return new vscode.Hover(markdown, symbol.range);
  }

  const keyword = keywordDocumentation[symbol.name];
  if (keyword) {
    return new vscode.Hover(new vscode.MarkdownString(keyword), symbol.range);
  }

  const primitive =
    primitiveTypes[symbol.name] ||
    (/^(?:u?int)(?:8|16|32|64)$/.test(symbol.name)
      ? ["A fixed-width integer with checked conversions and arithmetic.", `let value: ${symbol.name} = 1;`]
      : undefined);
  if (primitive) {
    const markdown = new vscode.MarkdownString();
    markdown.appendCodeblock(`final type ${symbol.name}`, "orni");
    markdown.appendMarkdown(`\nDeclared in \`orni:core\`\n\n${primitive[0]}\n\n`);
    markdown.appendCodeblock(primitive[1], "orni");
    return new vscode.Hover(markdown, symbol.range);
  }

  const functionDeclaration = findClosestDeclaration(
    findFunctions(source),
    symbol.name,
    usageOffset,
  );
  if (functionDeclaration) {
    const markdown = new vscode.MarkdownString();
    markdown.appendCodeblock(
      `func ${functionDeclaration.name}(${functionDeclaration.parameters.join(", ")}) -> ${functionDeclaration.returnType}`,
      "orni",
    );
    if (functionDeclaration.documentation) {
      markdown.appendMarkdown(`\n${functionDeclaration.documentation}`);
    }
    const hasAutomaticString =
      functionDeclaration.returnType === "string" ||
      functionDeclaration.parameters.some((parameter) => /:\s*string(?:\s*=|$)/.test(parameter));
    if (hasAutomaticString) {
      markdown.appendMarkdown(
        "\n\nBare `string` capacities are inferred from arguments, defaults, and the function body for each call.",
      );
    }
    return new vscode.Hover(markdown, symbol.range);
  }

  const variable = findClosestDeclaration(findVariables(source), symbol.name, usageOffset);
  if (variable) {
    const markdown = new vscode.MarkdownString();
    const inferredBareString =
      variable.typeName === "string" && variable.inferredType?.startsWith("string<");
    const resolvedType = inferredBareString
      ? variable.inferredType
      : variable.typeName || variable.inferredType;
    const annotation = resolvedType ? `: ${resolvedType}` : "";
    markdown.appendCodeblock(`let ${variable.name}${annotation}`, "orni");
    markdown.appendMarkdown(
      inferredBareString
        ? `\nDeclared as \`string\`; Orni inferred the allocation limit as \`${resolvedType}\` from the assigned value.`
        : variable.typeName
        ? `\nVariable explicitly declared as \`${variable.typeName}\`.`
        : resolvedType
          ? `\nInferred by Orni from the assigned value. No annotation is required.`
          : "\nVariable whose type is inferred from its value.",
    );
    return new vscode.Hover(markdown, symbol.range);
  }

  return undefined;
}

/** Provides parameter information while a function call is being written. */
function signatureHelpFor(document, position) {
  const source = document.getText();
  const prefix = source.slice(0, document.offsetAt(position));
  const call = prefix.match(/([A-Za-z_][A-Za-z0-9_]*)\s*\(([^()]*)$/s);
  if (!call) return undefined;

  const declaration = findFunctions(source).find(({ name }) => name === call[1]);
  if (!declaration) return undefined;

  const label = `func ${declaration.name}(${declaration.parameters.join(", ")}) -> ${declaration.returnType}`;
  const signature = new vscode.SignatureInformation(
    label,
    declaration.documentation || undefined,
  );
  signature.parameters = declaration.parameters.map(
    (parameter) => new vscode.ParameterInformation(parameter),
  );

  const help = new vscode.SignatureHelp();
  help.signatures = [signature];
  help.activeSignature = 0;
  help.activeParameter = Math.min(
    (call[2].match(/,/g) || []).length,
    Math.max(0, signature.parameters.length - 1),
  );
  return help;
}

/** Shows compiler-like inferred types without changing the source document. */
function inlayHintsFor(document, range) {
  return findVariables(document.getText())
    .filter(
      (variable) =>
        !variable.typeName &&
        variable.inferredType &&
        range.contains(document.positionAt(variable.offset)),
    )
    .map((variable) => {
      const position = document.positionAt(variable.offset + variable.name.length);
      const hint = new vscode.InlayHint(
        position,
        `: ${variable.inferredType}`,
        vscode.InlayHintKind.Type,
      );
      hint.tooltip = new vscode.MarkdownString(
        `Orni inferred \`${variable.inferredType}\` from the assigned value. The source code remains unchanged.`,
      );
      hint.paddingRight = true;
      return hint;
    });
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
      lines = sourcePath === document.uri.fsPath
        ? document.getText().split(/\r?\n/)
        : fs.readFileSync(sourcePath, "utf8").split(/\r?\n/);
    } catch {
      lines = document.getText().split(/\r?\n/);
    }
    const startLine = Math.min(Number(match[4]) - 1, Math.max(0, lines.length - 1));
    const endLine = Math.min(Number(match[6]) - 1, Math.max(0, lines.length - 1));
    const startColumn = Math.min(Number(match[5]) - 1, (lines[startLine] || "").length);
    const endColumn = Math.min(Number(match[7]) - 1, (lines[endLine] || "").length);
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
    diagnostic.code = /expects string<\d+>, but found string<\d+>/.test(match[8])
      ? "orni.stringCapacity"
      : match[2];
    return { uri, diagnostic };
  });
}

/** Publishes compiler diagnostics grouped by their source file. */
function setExternalCompilerDiagnostics(collection, entries, excludedUri) {
  const grouped = new Map();
  for (const { uri, diagnostic } of entries) {
    if (uri.toString() === excludedUri.toString()) continue;
    const key = uri.toString();
    const group = grouped.get(key) || { uri, diagnostics: [] };
    group.diagnostics.push(diagnostic);
    grouped.set(key, group);
  }
  for (const group of grouped.values()) {
    collection.set(group.uri, group.diagnostics);
  }
}

/** Produces fast non-blocking warnings without invoking the compiler. */
function lintDiagnostics(document) {
  const source = document.getText();
  const unusedImports = findUnusedImports(source).map((moduleImport) => {
    const start = document.positionAt(moduleImport.offset);
    const diagnostic = new vscode.Diagnostic(
      new vscode.Range(start, start.translate(0, moduleImport.length)),
      moduleImport.unusedCode === "ORNI-W002"
        ? `Selectively imported member '${moduleImport.unusedName}' is never used.`
        : `Module namespace '${moduleImport.unusedName}' is imported but never used.`,
      vscode.DiagnosticSeverity.Warning,
    );
    diagnostic.source = "Orni lint";
    diagnostic.code = moduleImport.unusedCode;
    return diagnostic;
  });
  const unusedVariables = findVariables(source).flatMap((variable) => {
    if (variable.name.startsWith("_")) return [];
    const uses = source.match(new RegExp(`\\b${variable.name}\\b`, "g")) || [];
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
  const unusedPrivateFunctions = findUnusedPrivateFunctions(source).map((declaration) => {
    const start = document.positionAt(declaration.offset);
    const diagnostic = new vscode.Diagnostic(
      new vscode.Range(start, start.translate(0, declaration.name.length)),
      `Private function '${declaration.name}' is never used.`,
      vscode.DiagnosticSeverity.Warning,
    );
    diagnostic.source = "Orni lint";
    diagnostic.code = "ORNI-W003";
    return diagnostic;
  });
  const boundedStrings = findBoundedStringViolations(source).map((violation) => {
    const start = document.positionAt(violation.offset);
    const end = document.positionAt(violation.offset + violation.length);
    const diagnostic = new vscode.Diagnostic(
      new vscode.Range(start, end),
      violation.message,
      vscode.DiagnosticSeverity.Error,
    );
    diagnostic.source = "Orni lint";
    diagnostic.code = "orni.stringCapacity";
    return diagnostic;
  });
  const boundedVariables = findBoundedVariableViolations(source).map((violation) => {
    const start = document.positionAt(violation.offset);
    const end = document.positionAt(violation.offset + violation.length);
    const diagnostic = new vscode.Diagnostic(
      new vscode.Range(start, end),
      violation.message,
      vscode.DiagnosticSeverity.Error,
    );
    diagnostic.source = "Orni lint";
    diagnostic.code = "orni.stringCapacity";
    return diagnostic;
  });
  return [
    ...boundedVariables,
    ...boundedStrings,
    ...unusedImports,
    ...unusedVariables,
    ...unusedPrivateFunctions,
  ];
}

/** Discovers public declarations that can be imported from nearby modules. */
async function workspacePublicMembers(document) {
  const moduleUris = await vscode.workspace.findFiles(
    "**/*.orni",
    "**/{.git,target,node_modules}/**",
    200,
  );
  const currentDirectory = path.dirname(document.uri.fsPath);
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
      for (const declaration of findFunctions(source).filter(({ name }) => !name.startsWith("_"))) {
        members.push({ ...declaration, modulePath, kind: "function" });
      }
      for (const declaration of findVariables(source).filter(({ name }) => !name.startsWith("_"))) {
        members.push({ ...declaration, modulePath, kind: "variable" });
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
  if (findImports(source).some(({ path: importedPath }) => importedPath === candidate.modulePath)) {
    return undefined;
  }
  const offset = importInsertionOffset(source);
  const suffix = offset === 0 ? "\n\n" : "\n";
  return vscode.TextEdit.insert(
    document.positionAt(offset),
    `import ${candidate.modulePath}.{${candidate.name}};${suffix}`,
  );
}

/** Creates context-aware fixes shown by the editor's Ctrl+. menu. */
async function codeActionsFor(document, context) {
  const actions = [];
  const source = document.getText();

  for (const diagnostic of context.diagnostics) {
    if (diagnostic.code === "orni.stringCapacity") {
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

    const missingName = diagnostic.message.match(/undefined variable '([A-Za-z_][A-Za-z0-9_]*)'/)?.[1];
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

/** Checks one saved file using the stricter reusable-module rules. */
async function checkModuleFromEditor(uri, diagnostics) {
  if (!uri) return;
  const document = await vscode.workspace.openTextDocument(uri);
  if (document.isDirty && !(await document.save())) return;
  const diagnosticKey = document.uri.toString();
  clearTimeout(diagnosticTimers.get(diagnosticKey));
  diagnosticTimers.delete(diagnosticKey);
  diagnosticProcesses.get(diagnosticKey)?.kill();
  const folder = vscode.workspace.getWorkspaceFolder(document.uri);
  if (!folder) {
    vscode.window.showWarningMessage("Open the Orni project folder before checking a module.");
    return;
  }

  await vscode.window.withProgress(
    {
      location: vscode.ProgressLocation.Notification,
      title: `Checking Orni module ${path.basename(document.uri.fsPath)}`,
    },
    () => new Promise((resolve) => {
      const modulePath = path
        .relative(folder.uri.fsPath, document.uri.fsPath)
        .split(path.sep)
        .join("/");
      const process = spawn(
        "cargo",
        ["run", "--quiet", "--target", "wasm32-wasip1", "--", "--check-module", modulePath],
        { cwd: folder.uri.fsPath, windowsHide: true },
      );
      let output = "";
      let failedToStart = false;
      process.stdout.on("data", (chunk) => (output += chunk));
      process.stderr.on("data", (chunk) => (output += chunk));
      process.on("error", (error) => {
        failedToStart = true;
        vscode.window.showErrorMessage(`Could not start the Orni checker: ${error.message}`);
        resolve();
      });
      process.on("close", (code) => {
        if (failedToStart) return;
        const entries = diagnosticFromCompiler(document, folder, output);
        if (code === 0) {
          const localDiagnostics = lintDiagnostics(document);
          const compilerDiagnostics = entries
            .filter(({ uri: target }) => target.toString() === document.uri.toString())
            .map(({ diagnostic }) => diagnostic)
            .filter(
              (compilerDiagnostic) =>
                !localDiagnostics.some(
                  (localDiagnostic) => localDiagnostic.code === compilerDiagnostic.code,
                ),
            );
          diagnostics.set(document.uri, [...compilerDiagnostics, ...localDiagnostics]);
          setExternalCompilerDiagnostics(diagnostics, entries, document.uri);
          vscode.window.showInformationMessage(
            `${path.basename(document.uri.fsPath)} is a valid reusable Orni module${entries.length ? ` with ${entries.length} warning(s)` : ""}.`,
          );
        } else {
          const current = entries
            .filter(({ uri: target }) => target.toString() === document.uri.toString())
            .map(({ diagnostic }) => diagnostic);
          if (current.length) diagnostics.set(document.uri, current);
          setExternalCompilerDiagnostics(diagnostics, entries, document.uri);
          vscode.window.showErrorMessage(
            entries[0]?.diagnostic.message || "The Orni module check failed.",
          );
        }
        resolve();
      });
    }),
  );
}

/** Checks the current in-memory source without executing its program. */
function updateDiagnostics(document, diagnostics) {
  if (document.languageId !== "orni") return;
  const localDiagnostics = lintDiagnostics(document);
  diagnostics.set(document.uri, localDiagnostics);
  const folder = vscode.workspace.getWorkspaceFolder(document.uri);
  if (!folder) return;
  const hasImports = findImports(document.getText()).length > 0;
  if (hasImports && document.isDirty) return;

  const key = document.uri.toString();
  for (const uri of compilerDiagnosticUris.get(key) || []) {
    if (uri.toString() !== document.uri.toString()) diagnostics.delete(uri);
  }
  compilerDiagnosticUris.delete(key);
  diagnosticProcesses.get(key)?.kill();
  const compilerArguments = hasImports
    ? [
        "run",
        "--quiet",
        "--target",
        "wasm32-wasip1",
        "--",
        "--check",
        path.relative(folder.uri.fsPath, document.uri.fsPath).split(path.sep).join("/"),
      ]
    : ["run", "--quiet", "--target", "wasm32-wasip1", "--", "--check-stdin"];
  const process = spawn(
    "cargo",
    compilerArguments,
    { cwd: folder.uri.fsPath, windowsHide: true },
  );
  diagnosticProcesses.set(key, process);
  let output = "";
  process.stdout.on("data", (chunk) => (output += chunk));
  process.stderr.on("data", (chunk) => (output += chunk));
  process.on("error", () => {
    if (diagnosticProcesses.get(key) !== process) return;
    diagnosticProcesses.delete(key);
    diagnostics.set(document.uri, localDiagnostics);
  });
  process.on("close", (code) => {
    if (diagnosticProcesses.get(key) !== process) return;
    diagnosticProcesses.delete(key);
    const compilerEntries = diagnosticFromCompiler(document, folder, output);
    const currentEntries = compilerEntries.filter(
      ({ uri }) => uri.toString() === document.uri.toString(),
    );
    const compilerOnly = currentEntries.map(({ diagnostic }) => diagnostic).filter(
      (compilerDiagnostic) =>
        !localDiagnostics.some(
          (localDiagnostic) =>
            localDiagnostic.code && localDiagnostic.code === compilerDiagnostic.code,
        ),
    );
    diagnostics.set(document.uri, [...compilerOnly, ...localDiagnostics]);
    setExternalCompilerDiagnostics(diagnostics, compilerEntries, document.uri);
    compilerDiagnosticUris.set(key, new Set(compilerEntries.map(({ uri }) => uri)));
  });
  process.stdin.end(hasImports ? undefined : document.getText());
}

/** Schedules diagnostics shortly after an Orni document changes. */
function scheduleDiagnostics(document, diagnostics) {
  const key = document.uri.toString();
  clearTimeout(diagnosticTimers.get(key));
  diagnosticTimers.set(
    key,
    setTimeout(() => {
      diagnosticTimers.delete(key);
      updateDiagnostics(document, diagnostics);
    }, 400),
  );
}

/** Resolves a symbol to its declaration or built-in documentation page. */
async function definitionFor(document, position) {
  const symbol = symbolAt(document, position);
  if (!symbol) return undefined;

  const backendDefinition = await backendDefinitionFor(symbol.name);
  if (backendDefinition) return backendDefinition;

  const source = document.getText();
  const usageOffset = document.offsetAt(position);
  const moduleImport = findImports(source).find(
    (candidate) =>
      usageOffset >= candidate.offset &&
      usageOffset <= candidate.offset + candidate.length,
  );
  if (moduleImport) {
    const modulePath = path.join(
      path.dirname(document.uri.fsPath),
      ...moduleImport.segments,
    ) + ".orni";
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
      const modulePath = path.join(
        path.dirname(document.uri.fsPath),
        ...imported.segments,
      ) + ".orni";
      try {
        const moduleDocument = await vscode.workspace.openTextDocument(modulePath);
        const moduleSource = moduleDocument.getText();
        const declaration =
          findFunctions(moduleSource).find(({ name }) => name === member) ||
          findVariables(moduleSource).find(({ name }) => name === member);
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
    findClosestDeclaration(findVariables(source), symbol.name, usageOffset);

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

/** Resolves one public member inside the module named by an import. */
async function importedMemberLocation(importingDocument, moduleImport, memberName) {
  const modulePath = path.join(
    path.dirname(importingDocument.uri.fsPath),
    ...moduleImport.segments,
  ) + ".orni";
  try {
    const moduleDocument = await vscode.workspace.openTextDocument(modulePath);
    const declaration =
      findFunctions(moduleDocument.getText()).find(({ name }) => name === memberName) ||
      findVariables(moduleDocument.getText()).find(({ name }) => name === memberName);
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

  const modulePath = path.join(
    path.dirname(document.uri.fsPath),
    ...imported.segments,
  ) + ".orni";
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
    return [...functions, ...variables];
  } catch {
    return undefined;
  }
}

/** Activates semantic navigation and documentation for Orni files. */
function activate(context) {
  const selector = { language: "orni", scheme: "file" };
  const diagnostics = vscode.languages.createDiagnosticCollection("orni");

  context.subscriptions.push(
    diagnostics,
    vscode.languages.registerHoverProvider(selector, {
      provideHover: hoverFor,
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
    vscode.workspace.onDidChangeTextDocument(({ document }) =>
      scheduleDiagnostics(document, diagnostics),
    ),
    vscode.workspace.onDidCloseTextDocument((document) => {
      const key = document.uri.toString();
      clearTimeout(diagnosticTimers.get(key));
      diagnosticProcesses.get(key)?.kill();
      diagnostics.delete(document.uri);
    }),
  );

  for (const document of vscode.workspace.textDocuments) {
    scheduleDiagnostics(document, diagnostics);
  }
}

/** Deactivates the extension. Registered resources are disposed by VS Code. */
function deactivate() {}

module.exports = { activate, deactivate };
