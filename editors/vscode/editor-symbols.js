const vscode = require("vscode");

const { resolveModule } = require("./project-support");

const path = require("node:path");

const { findClosestDeclaration, findContracts, findFunctions, findImports, findVariables, findProviders, identifierPattern } = require("./language-support");

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
  contract: "Declares the callable behavior required from an injected dependency.",
  provider: "Implements every method required by a dependency contract.",
  for: "Connects a provider declaration to the contract it implements.",
  bind: "Selects the provider that Orni injects for a contract.",
  to: "Connects a contract binding to its selected provider.",
};

const primitiveTypes = {
  string: ["Dynamically sized UTF-8 text.", 'let name = "Orni"; // inferred as string<4>'],
  int: ["A signed 64-bit integer with checked arithmetic.", "let workers: int = 8;"],
  double: ["A 64-bit floating-point number.", "let ratio: double = 0.75;"],
  bool: ["A logical value that is either true or false.", "let enabled: bool = true;"],
  char: ["One Unicode scalar value.", "let initial: char = 'O';"],
  unit: ["The absence of a reusable value.", "func log() -> unit { print \"done\"; }"],
};

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
    const modulePath = resolveModule(document.uri.fsPath, moduleImport.segments);
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

  const contract = findContracts(source).find(({ name }) => name === symbol.name);
  if (contract) {
    const markdown = new vscode.MarkdownString();
    markdown.appendCodeblock(`contract ${contract.name}`, "orni");
    markdown.appendMarkdown("\nInjected dependency contract. Required methods:");
    for (const method of contract.methods) {
      markdown.appendMarkdown(
        `\n- \`func ${method.name}(${method.parameters.join(", ")}) -> ${method.returnType}\``,
      );
    }
    return new vscode.Hover(markdown, symbol.range);
  }

  const provider = findProviders(source).find(({ name }) => name === symbol.name);
  if (provider) {
    const markdown = new vscode.MarkdownString();
    const dependencies = provider.dependencies.length === 0
      ? ""
      : `(${provider.dependencies.join(", ")})`;
    markdown.appendCodeblock(
      `provider ${provider.name}${dependencies} for ${provider.contract}`,
      "orni",
    );
    markdown.appendMarkdown(
      `\nConcrete implementation available for the \`${provider.contract}\` contract.`,
    );
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

module.exports = { hoverFor, signatureHelpFor, inlayHintsFor, symbolAt, backendDefinitionFor };
