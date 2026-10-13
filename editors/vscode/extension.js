const vscode = require("vscode");
const { spawn } = require("node:child_process");
const {
  findClosestDeclaration,
  findBoundedStringViolations,
  findBoundedVariableViolations,
  findFunctions,
  findStringCapacityAnnotation,
  findVariables,
  identifierPattern,
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
function hoverFor(document, position) {
  const symbol = symbolAt(document, position);
  if (!symbol) return undefined;

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

  const source = document.getText();
  const usageOffset = document.offsetAt(position);
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
function diagnosticFromCompiler(document, output) {
  const match = output.match(
    /(?:Lexical|Syntax|Semantic) error at line (\d+), column (\d+): ([^\r\n]+)/,
  );
  if (!match) return [];

  const line = Math.min(Number(match[1]) - 1, Math.max(0, document.lineCount - 1));
  const lineText = document.lineAt(line).text;
  const column = Math.min(Number(match[2]) - 1, lineText.length);
  const end = Math.min(column + 1, lineText.length);
  const diagnostic = new vscode.Diagnostic(
    new vscode.Range(line, column, line, end),
    match[3],
    vscode.DiagnosticSeverity.Error,
  );
  diagnostic.source = "Orni compiler";
  if (/expects string<\d+>, but found string<\d+>/.test(match[3])) {
    diagnostic.code = "orni.stringCapacity";
  }
  return [diagnostic];
}

/** Produces fast non-blocking warnings without invoking the compiler. */
function lintDiagnostics(document) {
  const source = document.getText();
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
  return [...boundedVariables, ...boundedStrings, ...unusedVariables];
}

/** Creates context-aware fixes shown by the editor's Ctrl+. menu. */
function codeActionsFor(document, context) {
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
  }

  return actions;
}

/** Checks the current in-memory source without executing its program. */
function updateDiagnostics(document, diagnostics) {
  if (document.languageId !== "orni") return;
  const localDiagnostics = lintDiagnostics(document);
  diagnostics.set(document.uri, localDiagnostics);
  const folder = vscode.workspace.getWorkspaceFolder(document.uri);
  if (!folder) return;

  const key = document.uri.toString();
  diagnosticProcesses.get(key)?.kill();
  const process = spawn(
    "cargo",
    ["run", "--quiet", "--target", "wasm32-wasip1", "--", "--check-stdin"],
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
    const compilerDiagnostics = code === 0 ? [] : diagnosticFromCompiler(document, output);
    const compilerOnly = compilerDiagnostics.filter(
      (compilerDiagnostic) =>
        !localDiagnostics.some(
          (localDiagnostic) =>
            localDiagnostic.code && localDiagnostic.code === compilerDiagnostic.code,
        ),
    );
    diagnostics.set(document.uri, [...compilerOnly, ...localDiagnostics]);
  });
  process.stdin.end(document.getText());
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
  const declaration =
    findClosestDeclaration(findFunctions(source), symbol.name, usageOffset) ||
    findClosestDeclaration(findVariables(source), symbol.name, usageOffset);

  if (!declaration) return undefined;
  return new vscode.Location(document.uri, document.positionAt(declaration.offset));
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
      { providedCodeActionKinds: [vscode.CodeActionKind.QuickFix] },
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
