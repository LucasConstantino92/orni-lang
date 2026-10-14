const identifierPattern = /[A-Za-z_][A-Za-z0-9_]*/;

/** Masks comments and literals while keeping source offsets and line breaks intact. */
function syntaxMask(source) {
  return source.replace(/\/\/[^\r\n]*|"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'/g,
    (text) => text.replace(/[^\r\n]/g, " "));
}

/** Splits arguments without treating quoted commas or nested calls as separators. */
function splitParameters(source) {
  const mask = syntaxMask(source);
  const result = [];
  let depth = 0;
  let start = 0;
  for (let index = 0; index < mask.length; index++) {
    if ("([{".includes(mask[index])) depth++;
    if (")]}".includes(mask[index])) depth--;
    if (mask[index] === "," && depth === 0) {
      result.push(source.slice(start, index).trim());
      start = index + 1;
    }
  }
  result.push(source.slice(start).trim());
  return result.filter(Boolean);
}

/** Finds the matching delimiter in a masked source document. */
function closingDelimiter(mask, start, open, close) {
  let depth = 0;
  for (let index = start; index < mask.length; index++) {
    if (mask[index] === open) depth++;
    if (mask[index] === close && --depth === 0) return index;
  }
  return -1;
}

/** Filters module exports without exposing function locals or provider methods. */
function topLevelDeclarations(source, declarations) {
  const mask = syntaxMask(source);
  const depths = new Int32Array(mask.length + 1);
  for (let index = 0; index < mask.length; index++) {
    depths[index + 1] = depths[index] + (mask[index] === "{" ? 1 : mask[index] === "}" ? -1 : 0);
  }
  return declarations.filter(({ offset }) => depths[offset] === 0);
}

/** Finds dependency contract declarations and their required methods. */
function findContracts(source) {
  const mask = syntaxMask(source);
  const pattern = /\bcontract[ \t]+([A-Za-z_][A-Za-z0-9_]*)\s*\{/g;
  return Array.from(mask.matchAll(pattern), (match) => {
    const opening = match.index + match[0].lastIndexOf("{");
    const closing = closingDelimiter(mask, opening, "{", "}");
    return {
      name: match[1],
      methods: findFunctions(source.slice(opening + 1, closing < 0 ? source.length : closing)),
      offset: match.index + match[0].indexOf(match[1]),
    };
  });
}

/** Finds provider declarations and their implemented contracts. */
function findProviders(source) {
  const pattern = /\bprovider\s+([A-Za-z_][A-Za-z0-9_]*)\s*(?:\(([^)]*)\)\s*)?for\s+([A-Za-z_][A-Za-z0-9_.]*)/g;
  return Array.from(syntaxMask(source).matchAll(pattern), (match) => ({
    name: match[1],
    dependencies: (match[2] ?? "")
      .split(",")
      .map((dependency) => dependency.trim())
      .filter(Boolean),
    contract: match[3],
    offset: match.index + match[0].indexOf(match[1]),
  }));
}

/** Finds dependency bindings declared by an executable Orni entry file. */
function findBindings(source) {
  const pattern = /\bbind\s+([A-Za-z_][A-Za-z0-9_.]*)\s+to\s+([A-Za-z_][A-Za-z0-9_.]*)\s*;/g;
  return Array.from(syntaxMask(source).matchAll(pattern), (match) => ({
    contract: match[1],
    provider: match[2],
    offset: match.index,
    length: match[0].length,
  }));
}

/** Converts a public Orni type name into a conventional module file name. */
function snakeCaseTypeName(name) {
  return name
    .replace(/([a-z0-9])([A-Z])/g, "$1_$2")
    .replace(/([A-Z])([A-Z][a-z])/g, "$1_$2")
    .toLowerCase();
}

/** Returns a safe placeholder expression for a generated provider method. */
function defaultValueForType(typeName) {
  if (typeName === "bool") return "false";
  if (typeName === "double") return "0.0";
  if (typeName === "char") return "' '";
  if (/^string(?:<\d+>)?$/.test(typeName)) return '""';
  if (/^(?:u?int)(?:8|16|32|64)?$/.test(typeName)) return "0";
  return undefined;
}

/** Generates an editable provider implementation from a contract declaration. */
function providerSource(contract, providerName, contractModulePath) {
  const importLine = contractModulePath
    ? `import ${contractModulePath}.{${contract.name}};\n\n`
    : "";
  const methods = contract.methods.map((method) => {
    const value = defaultValueForType(method.returnType);
    const body = method.returnType === "unit"
      ? ""
      : value === undefined
        ? "        // TODO: Return a value compatible with this contract method.\n"
        : `        return ${value};\n`;
    const documentation = method.documentation
      ? method.documentation.split("\n").map((line) => `    /// ${line}\n`).join("")
      : "";
    return `${documentation}    func ${method.name}(${method.parameters.join(", ")}) -> ${method.returnType} {\n${body}    }`;
  }).join("\n\n");
  return `${importLine}/// Provides the default ${contract.name} implementation.\nprovider ${providerName} for ${contract.name} {\n${methods}\n}\n`;
}

/** Finds documented functions using balanced parameters and literal-aware scanning. */
function findFunctions(source) {
  const mask = syntaxMask(source);
  const pattern = /\bfunc\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(/g;
  const functions = [];
  for (const match of mask.matchAll(pattern)) {
    const opening = match.index + match[0].lastIndexOf("(");
    const closing = closingDelimiter(mask, opening, "(", ")");
    if (closing < 0) continue;
    const result = mask.slice(closing + 1).match(/^\s*->\s*([A-Za-z_][A-Za-z0-9_]*(?:\s*<\s*\d+\s*>)?)/);
    if (!result) continue;
    const docs = source.slice(0, match.index).match(/((?:[ \t]*\/\/\/[^\r\n]*\r?\n)+)[ \t]*$/)?.[1] || "";
    functions.push({
      name: match[1],
      parameters: splitParameters(source.slice(opening + 1, closing)),
      returnType: result[1].replace(/\s/g, ""),
      documentation: docs.split(/\r?\n/).map((line) => line.replace(/^\s*\/\/\/\s?/, "").trimEnd()).filter(Boolean).join("\n"),
      offset: match.index + match[0].indexOf(match[1]),
    });
  }
  return functions;
}

/** Finds dotted file-module imports in an Orni source document. */
function findImports(source) {
  const pattern = /^[ \t]*import\s+([A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*)(?:\.\{([^}]*)\})?(?:\s+as\s+([A-Za-z_][A-Za-z0-9_]*))?\s*;/gm;
  return Array.from(syntaxMask(source).matchAll(pattern), (match) => {
    const selectionOffset = match[2] === undefined
      ? undefined
      : match.index + match[0].indexOf(`{${match[2]}}`) + 1;
    let memberSearchOffset = 0;
    const members = match[2] === undefined
      ? []
      : match[2].split(",").map((rawMember) => {
          const name = rawMember.trim();
          const relativeOffset = match[2].indexOf(name, memberSearchOffset);
          memberSearchOffset = relativeOffset + name.length;
          return {
            name,
            offset: selectionOffset + relativeOffset,
            length: name.length,
          };
        }).filter(({ name }) => name.length > 0);
    return {
      path: match[1],
      segments: match[1].split("."),
      alias: match[3],
      namespace: members.length ? undefined : match[3] || match[1].split(".").at(-1),
      members,
      offset: match.index + match[0].indexOf(match[1]),
      length: match[1].length,
      statementOffset: match.index,
      statementLength: match[0].length,
    };
  });
}

/** Finds module namespaces that are imported but never referenced. */
function findUnusedImports(source) {
  return findImports(source).flatMap((moduleImport) => {
    const withoutDeclaration =
      source.slice(0, moduleImport.statementOffset) +
      source.slice(moduleImport.statementOffset + moduleImport.statementLength);
    if (moduleImport.members.length) {
      return moduleImport.members
        .filter(({ name }) => !new RegExp(`\\b${name}\\b`).test(withoutDeclaration))
        .map((member) => ({
          ...moduleImport,
          unusedName: member.name,
          unusedCode: "ORNI-W002",
          offset: member.offset,
          length: member.length,
        }));
    }
    const namespaceUse = new RegExp(`\\b${moduleImport.namespace}\\s*\\.`);
    return namespaceUse.test(withoutDeclaration)
      ? []
      : [{
          ...moduleImport,
          unusedName: moduleImport.namespace,
          unusedCode: "ORNI-W001",
        }];
  });
}

/** Finds underscore-prefixed functions that are never called. */
function findUnusedPrivateFunctions(source) {
  return findFunctions(source).filter(({ name }) => {
    if (!name.startsWith("_")) return false;
    return (source.match(new RegExp(`\\b${name}\\b`, "g")) || []).length === 1;
  });
}

/** Returns import statements in their deterministic alphabetical order. */
function sortedImportStatements(source) {
  return findImports(source)
    .map((moduleImport) => source.slice(
      moduleImport.statementOffset,
      moduleImport.statementOffset + moduleImport.statementLength,
    ))
    .sort((left, right) => left.localeCompare(right));
}

/** Chooses the position where an automatically generated import should be inserted. */
function importInsertionOffset(source) {
  const imports = findImports(source);
  if (!imports.length) return 0;
  const last = imports.at(-1);
  let offset = last.statementOffset + last.statementLength;
  if (source.slice(offset, offset + 2) === "\r\n") return offset + 2;
  if (source[offset] === "\n") return offset + 1;
  return offset;
}

/** Finds every variable declaration in an Orni source document. */
function findVariables(source) {
  const pattern = /\blet[ \t]+([A-Za-z_][A-Za-z0-9_]*)(?:[ \t]*:[ \t]*([A-Za-z_][A-Za-z0-9_]*(?:<\d+>)?))?[ \t]*=/g;
  const mask = syntaxMask(source);
  return Array.from(mask.matchAll(pattern), (match) => ({
    name: match[1],
    typeName: match[2],
    inferredType: inferExpressionType(source.slice(match.index + match[0].length, mask.indexOf(";", match.index) < 0 ? source.length : mask.indexOf(";", match.index)).trim()),
    offset: match.index + match[0].indexOf(match[1]),
  }));
}

/** Infers simple literal types for fast editor hovers. */
function inferExpressionType(expression) {
  if (/^"(?:\\.|[^"\\])*"$/.test(expression)) {
    try {
      return `string<${Buffer.byteLength(decodeString(expression), "utf8")}>`;
    } catch {
      return undefined;
    }
  }
  if (/^'(?:\\[nt'\\]|[^'\r\n])'$/u.test(expression)) return "char";
  if (/^(?:true|false)$/.test(expression)) return "bool";
  if (/^-?\d+\.\d+$/.test(expression)) return "double";
  if (/^-?\d+$/.test(expression)) return inferIntegerType(expression);
  return undefined;
}

/** Decodes exactly the escapes accepted by the Orni lexer. */
function decodeString(literal) {
  if (!/^"(?:\\[nt"\\]|[^"\\])*"$/.test(literal)) throw new Error("Invalid Orni string escape.");
  return literal.slice(1, -1).replace(/\\([nt"\\])/g, (_, escape) => ({ n: "\n", t: "\t", '"': '"', "\\": "\\" })[escape]);
}

/** Chooses the smallest signed range, falling back to uint64 for larger positives. */
function inferIntegerType(expression) {
  try {
    const value = BigInt(expression);
    if (value >= -128n && value <= 127n) return "int8";
    if (value >= -32768n && value <= 32767n) return "int16";
    if (value >= -2147483648n && value <= 2147483647n) return "int32";
    if (value >= -9223372036854775808n && value <= 9223372036854775807n) return "int";
    if (value >= 0n && value <= 18446744073709551615n) return "uint64";
    return undefined;
  } catch {
    return undefined;
  }
}

/** Finds bounded variable declarations whose string literal does not fit. */
function findBoundedVariableViolations(source) {
  const pattern = /\blet\s+[A-Za-z_][A-Za-z0-9_]*\s*:\s*string<(\d+)>\s*=\s*("(?:\\.|[^"\\])*")/g;
  const violations = [];
  for (const match of source.matchAll(pattern)) {
    try {
      const maximum = Number(match[1]);
      const actual = Buffer.byteLength(decodeString(match[2]), "utf8");
      if (actual > maximum) {
        const literalOffset = match.index + match[0].lastIndexOf(match[2]);
        violations.push({
          offset: literalOffset,
          length: match[2].length,
          message: `String value requires ${actual} UTF-8 bytes, but string<${maximum}> allows at most ${maximum}.`,
        });
      }
    } catch {
      // The compiler reports malformed literals with more precise context.
    }
  }
  return violations;
}

/** Locates a bounded string annotation on the diagnostic's source line. */
function findStringCapacityAnnotation(source, offset, capacity) {
  const lineStart = source.lastIndexOf("\n", Math.max(0, offset - 1)) + 1;
  const nextLine = source.indexOf("\n", offset);
  const lineEnd = nextLine === -1 ? source.length : nextLine;
  const line = source.slice(lineStart, lineEnd);
  const match = line.match(new RegExp(`string\\s*<\\s*${capacity}\\s*>`));
  if (!match) return undefined;
  return {
    offset: lineStart + match.index,
    length: match[0].length,
  };
}

/** Finds string literals that exceed a function parameter's declared capacity. */
function findBoundedStringViolations(source) {
  const violations = [];
  for (const declaration of findFunctions(source)) {
    const limits = declaration.parameters.map((parameter) => {
      const match = parameter.match(/^[A-Za-z_][A-Za-z0-9_]*\s*:\s*string<(\d+)>/);
      return match ? Number(match[1]) : undefined;
    });
    if (!limits.some((limit) => limit !== undefined)) continue;

    const calls = new RegExp(`\\b${declaration.name}\\s*\\(([^()]*)\\)`, "g");
    for (const call of source.matchAll(calls)) {
      const prefix = source.slice(Math.max(0, call.index - 8), call.index);
      if (/func\s+$/.test(prefix)) continue;

      const argumentsSource = call[1];
      const argumentsOffset = call.index + call[0].indexOf(argumentsSource);
      let searchOffset = 0;
      for (const [index, rawArgument] of argumentsSource.split(",").entries()) {
        const argument = rawArgument.trim();
        const localOffset = argumentsSource.indexOf(argument, searchOffset);
        searchOffset = localOffset + argument.length;
        const maximum = limits[index];
        if (maximum === undefined || !/^"(?:\\.|[^"\\])*"$/.test(argument)) continue;
        try {
          const actual = Buffer.byteLength(decodeString(argument), "utf8");
          if (actual > maximum) {
            violations.push({
              offset: argumentsOffset + localOffset,
              length: argument.length,
              message: `String argument requires ${actual} UTF-8 bytes, but string<${maximum}> allows at most ${maximum}.`,
            });
          }
        } catch {
          // The compiler reports malformed string literals with better context.
        }
      }
    }
  }
  return violations;
}

/** Returns the closest declaration visible before a symbol use. */
function findClosestDeclaration(declarations, name, usageOffset) {
  return declarations
    .filter((declaration) => declaration.name === name && declaration.offset <= usageOffset)
    .sort((left, right) => right.offset - left.offset)[0];
}

module.exports = {
  findClosestDeclaration,
  findBindings,
  findBoundedStringViolations,
  findBoundedVariableViolations,
  findContracts,
  findFunctions,
  findImports,
  findProviders,
  findStringCapacityAnnotation,
  findUnusedImports,
  findUnusedPrivateFunctions,
  findVariables,
  importInsertionOffset,
  inferExpressionType,
  inferIntegerType,
  identifierPattern,
  syntaxMask,
  splitParameters,
  topLevelDeclarations,
  providerSource,
  snakeCaseTypeName,
  sortedImportStatements,
};
