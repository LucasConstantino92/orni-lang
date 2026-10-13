const identifierPattern = /[A-Za-z_][A-Za-z0-9_]*/;

/** Finds all documented function declarations in an Orni source document. */
function findFunctions(source) {
  const pattern = /((?:^[ \t]*\/\/\/[^\r\n]*(?:\r?\n|$))*)^[ \t]*func[ \t]+([A-Za-z_][A-Za-z0-9_]*)[ \t]*\(([^)]*)\)[ \t]*->[ \t]*([A-Za-z_][A-Za-z0-9_]*(?:<\d+>)?)/gm;
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

/** Finds every variable declaration in an Orni source document. */
function findVariables(source) {
  const pattern = /\blet[ \t]+([A-Za-z_][A-Za-z0-9_]*)(?:[ \t]*:[ \t]*([A-Za-z_][A-Za-z0-9_]*(?:<\d+>)?))?[ \t]*=[ \t]*([^;]+)/g;
  return Array.from(source.matchAll(pattern), (match) => ({
    name: match[1],
    typeName: match[2],
    inferredType: match[2] ? undefined : inferExpressionType(match[3].trim()),
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
  if (/^-?\d+$/.test(expression)) return "int";
  return undefined;
}

/** Returns the closest declaration visible before a symbol use. */
function findClosestDeclaration(declarations, name, usageOffset) {
  return declarations
    .filter((declaration) => declaration.name === name && declaration.offset <= usageOffset)
    .sort((left, right) => right.offset - left.offset)[0];
}

module.exports = {
  findClosestDeclaration,
  findFunctions,
  findVariables,
  inferExpressionType,
  identifierPattern,
};
