const identifierPattern = /[A-Za-z_][A-Za-z0-9_]*/;

/** Finds all documented function declarations in an Orni source document. */
function findFunctions(source) {
  const pattern = /((?:^[ \t]*\/\/\/[^\r\n]*(?:\r?\n|$))*)^[ \t]*(?:pub[ \t]+)?func[ \t]+([A-Za-z_][A-Za-z0-9_]*)[ \t]*\(([^)]*)\)[ \t]*->[ \t]*([A-Za-z_][A-Za-z0-9_]*(?:<\d+>)?)/gm;
  const functions = [];

  for (const match of source.matchAll(pattern)) {
    const name = match[2];
    const nameOffset = match.index + match[0].lastIndexOf(name);
    const documentation = match[1]
      .split(/\r?\n/)
      .map((line) => line.replace(/^\s*\/\/\/\s?/, "").trimEnd())
      .filter(Boolean)
      .join("\n");

    functions.push({
      name,
      parameters: match[3]
        .split(",")
        .map((parameter) => parameter.trim())
        .filter(Boolean),
      returnType: match[4],
      documentation,
      offset: nameOffset,
    });
  }

  return functions;
}

/** Finds dotted file-module imports in an Orni source document. */
function findImports(source) {
  const pattern = /^[ \t]*import\s+([A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*)(?:\.\{([^}]*)\})?(?:\s+as\s+([A-Za-z_][A-Za-z0-9_]*))?\s*;/gm;
  return Array.from(source.matchAll(pattern), (match) => {
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

/** Finds every variable declaration in an Orni source document. */
function findVariables(source) {
  const pattern = /\blet[ \t]+([A-Za-z_][A-Za-z0-9_]*)(?:[ \t]*:[ \t]*([A-Za-z_][A-Za-z0-9_]*(?:<\d+>)?))?[ \t]*=[ \t]*([^;]+)/g;
  return Array.from(source.matchAll(pattern), (match) => ({
    name: match[1],
    typeName: match[2],
    inferredType: inferExpressionType(match[3].trim()),
    offset: match.index + match[0].lastIndexOf(match[1]),
  }));
}

/** Infers simple literal types for fast editor hovers. */
function inferExpressionType(expression) {
  if (/^"(?:\\.|[^"\\])*"$/.test(expression)) {
    try {
      return `string<${Buffer.byteLength(JSON.parse(expression), "utf8")}>`;
    } catch {
      return undefined;
    }
  }
  if (/^'(?:\\[nt'\\]|[^'\r\n])'$/.test(expression)) return "char";
  if (/^(?:true|false)$/.test(expression)) return "bool";
  if (/^-?\d+\.\d+$/.test(expression)) return "double";
  if (/^-?\d+$/.test(expression)) return inferIntegerType(expression);
  return undefined;
}

/** Chooses the smallest signed integer type that safely contains a literal. */
function inferIntegerType(expression) {
  try {
    const value = BigInt(expression);
    if (value >= -128n && value <= 127n) return "int8";
    if (value >= -32768n && value <= 32767n) return "int16";
    if (value >= -2147483648n && value <= 2147483647n) return "int32";
    return "int";
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
      const actual = Buffer.byteLength(JSON.parse(match[2]), "utf8");
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
          const actual = Buffer.byteLength(JSON.parse(argument), "utf8");
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
  findBoundedStringViolations,
  findBoundedVariableViolations,
  findFunctions,
  findImports,
  findStringCapacityAnnotation,
  findUnusedImports,
  findVariables,
  inferExpressionType,
  inferIntegerType,
  identifierPattern,
};
