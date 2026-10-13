const assert = require("node:assert/strict");
const test = require("node:test");
const {
  findClosestDeclaration,
  findFunctions,
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
