const assert = require("node:assert/strict");
const test = require("node:test");
const {
  findClosestDeclaration,
  findBoundedStringViolations,
  findBoundedVariableViolations,
  findFunctions,
  findStringCapacityAnnotation,
  findVariables,
} = require("../language-support");

test("finds documented functions", () => {
  const source = `/// Adds two values.\nfunc add(left: int, right: int = 1) -> int { return left + right; }`;
  const [definition] = findFunctions(source);

  assert.equal(definition.name, "add");
  assert.deepEqual(definition.parameters, ["left: int", "right: int = 1"]);
  assert.equal(definition.returnType, "int");
  assert.equal(definition.documentation, "Adds two values.");
  assert.equal(source.slice(definition.offset, definition.offset + 3), "add");
});

test("finds the closest preceding variable", () => {
  const source = "let value: int = 1; print value; let value = 2; print value;";
  const declarations = findVariables(source);
  const usageOffset = source.lastIndexOf("value");
  const declaration = findClosestDeclaration(declarations, "value", usageOffset);

  assert.equal(declaration.offset, declarations[1].offset);
  assert.equal(declarations[0].typeName, "int");
  assert.equal(declarations[1].typeName, undefined);
});

test("infers bounded strings from UTF-8 literals", () => {
  const [ascii, unicode, explicit] = findVariables(
    'let name = "Lucas"; let greeting = "Olá"; let status: string = "Ready";',
  );

  assert.equal(ascii.inferredType, "string<5>");
  assert.equal(unicode.inferredType, "string<4>");
  assert.equal(explicit.typeName, "string");
  assert.equal(explicit.inferredType, "string<5>");
});

test("infers the smallest safe signed integer type", () => {
  const declarations = findVariables(
    "let retries = 3; let port = 8080; let users = 250000; let distance = 9000000000;",
  );

  assert.deepEqual(
    declarations.map(({ inferredType }) => inferredType),
    ["int8", "int16", "int32", "int"],
  );
});

test("reports string arguments that exceed parameter capacity", () => {
  const source = `
func greet(name: string<32>, greeting: string<16>) -> string<64> {
  return greeting + name;
}
print greet("Orni", "Welcome to the Orni world");
`;
  const [violation] = findBoundedStringViolations(source);

  assert.equal(violation.message.includes("25 UTF-8 bytes"), true);
  assert.equal(source.slice(violation.offset, violation.offset + violation.length), '"Welcome to the Orni world"');
});

test("reports bounded variable literals that exceed their capacity", () => {
  const source = 'let name: string<4> = "Lucas";';
  const [violation] = findBoundedVariableViolations(source);

  assert.equal(violation.message.includes("requires 5 UTF-8 bytes"), true);
  assert.equal(source.slice(violation.offset, violation.offset + violation.length), '"Lucas"');
});

test("locates a bounded string annotation for editor quick fixes", () => {
  const source = 'let name: string<4> = "Lucas";';
  const annotation = findStringCapacityAnnotation(source, source.indexOf("name"), 4);

  assert.equal(source.slice(annotation.offset, annotation.offset + annotation.length), "string<4>");
  assert.equal(findStringCapacityAnnotation(source, source.indexOf("name"), 8), undefined);
});
