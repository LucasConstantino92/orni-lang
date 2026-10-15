const assert = require("node:assert/strict");
const test = require("node:test");
const {
  findClosestDeclaration,
  findBindings,
  findBoundedStringViolations,
  findBoundedVariableViolations,
  findContracts,
  findFunctionParameters,
  findFunctions,
  findImports,
  findStringCapacityAnnotation,
  findUnusedImports,
  findUnusedPrivateFunctions,
  findVariables,
  findProviders,
  findModels,
  findStores,
  importInsertionOffset,
  providerSource,
  inferIntegerType,
  topLevelDeclarations,
  snakeCaseTypeName,
  sortedImportStatements,
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

test("finds contracts and providers", () => {
  const source = `
contract Logger {
  func log(message: string) -> unit;
}
provider ConsoleLogger for Logger {
  func log(message: string) -> unit { print message; }
}
`;
  const [contract] = findContracts(source);
  const [provider] = findProviders(source);

  assert.equal(contract.name, "Logger");
  assert.deepEqual(contract.methods.map(({ name }) => name), ["log"]);
  assert.deepEqual(provider, {
    name: "ConsoleLogger",
    dependencies: [],
    contract: "Logger",
    offset: source.indexOf("ConsoleLogger"),
  });
});

test("finds provider dependencies", () => {
  const source = `provider ConsoleAudit(logger: Logger, clock: Clock) for Audit {}`;
  const [provider] = findProviders(source);

  assert.deepEqual(provider.dependencies, ["logger: Logger", "clock: Clock"]);
  assert.equal(provider.contract, "Audit");
});

test("finds dependency bindings", () => {
  const source = "bind Logger to ConsoleLogger;\nbind Audit to ConsoleAudit;";

  assert.deepEqual(
    findBindings(source).map(({ contract, provider }) => ({ contract, provider })),
    [
      { contract: "Logger", provider: "ConsoleLogger" },
      { contract: "Audit", provider: "ConsoleAudit" },
    ],
  );
});

test("generates a provider skeleton from every contract method", () => {
  const [contract] = findContracts(`
contract Clock {
  func now() -> int;
  func ready() -> bool;
  func reset() -> unit;
}
`);
  const generated = providerSource(contract, "SystemClock", "contracts.clock");

  assert.match(generated, /import contracts\.clock\.\{Clock\};/);
  assert.match(generated, /provider SystemClock for Clock/);
  assert.match(generated, /func now\(\) -> int \{\n        return 0;/);
  assert.match(generated, /func ready\(\) -> bool \{\n        return false;/);
  assert.match(generated, /func reset\(\) -> unit \{\n    \}/);
  assert.equal(snakeCaseTypeName("HTTPClock"), "http_clock");
});

test("finds functions and aliased dotted module imports", () => {
  const source = "import services.greeting as welcome;\nfunc greet(name: string) -> string { return name; }";
  const [moduleImport] = findImports(source);
  const [definition] = findFunctions(source);

  assert.deepEqual(moduleImport.segments, ["services", "greeting"]);
  assert.equal(moduleImport.alias, "welcome");
  assert.equal(moduleImport.namespace, "welcome");
  assert.equal(source.slice(moduleImport.offset, moduleImport.offset + moduleImport.length), "services.greeting");
  assert.equal(definition.name, "greet");
});

test("finds model, store, and function parameter declarations", () => {
  const source = `
model User {
  id: uuid;
}
store users: User;
func create_user(input: User) -> response {
  return users.create(input);
}
`;

  const [model] = findModels(source);
  const [store] = findStores(source);
  const [parameter] = findFunctionParameters(source);

  assert.equal(model.name, "User");
  assert.equal(source.slice(model.offset, model.offset + model.name.length), "User");
  assert.deepEqual({ name: store.name, model: store.model }, { name: "users", model: "User" });
  assert.equal(parameter.name, "input");
  assert.equal(parameter.typeName, "User");
  assert(source.indexOf("input);") <= parameter.scopeEnd);
});

test("finds selectively imported module members", () => {
  const source = "import services.greeting.{greet, framework_name};\nprint greet(\"Orni\");";
  const [moduleImport] = findImports(source);

  assert.equal(moduleImport.path, "services.greeting");
  assert.equal(moduleImport.namespace, undefined);
  assert.deepEqual(moduleImport.members.map(({ name }) => name), ["greet", "framework_name"]);
  assert.equal(
    source.slice(moduleImport.members[1].offset, moduleImport.members[1].offset + moduleImport.members[1].length),
    "framework_name",
  );
});

test("reports only module imports whose namespace is unused", () => {
  const source = `
import services.greeting as welcome;
import services.logging;

print welcome.greet("Orni");
`;
  const [unused] = findUnusedImports(source);

  assert.equal(unused.namespace, "logging");
  assert.equal(source.slice(unused.offset, unused.offset + unused.length), "services.logging");
});

test("reports each unused selectively imported member", () => {
  const source = `
import services.greeting.{greet, framework_name};
print greet("Orni");
`;
  const [unused] = findUnusedImports(source);

  assert.equal(unused.unusedCode, "ORNI-W002");
  assert.equal(unused.unusedName, "framework_name");
  assert.equal(source.slice(unused.offset, unused.offset + unused.length), "framework_name");
});

test("reports only unused private functions", () => {
  const source = `
func _unused() -> unit { print "unused"; }
func _used() -> unit { print "used"; }
func start() -> unit { _used(); }
`;

  assert.deepEqual(
    findUnusedPrivateFunctions(source).map(({ name }) => name),
    ["_unused"],
  );
});

test("sorts imports and inserts new imports after the existing block", () => {
  const source = "import services.logging;\nimport services.greeting;\n\nprint logging.name;";

  assert.deepEqual(sortedImportStatements(source), [
    "import services.greeting;",
    "import services.logging;",
  ]);
  assert.equal(source.slice(importInsertionOffset(source)), "\nprint logging.name;");
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

test("ignores fake declarations in comments and literals and handles quoted delimiters", () => {
  const source = `// func fake() -> unit {}\nlet text = "contract Fake {}";\nfunc greet(name: string = "a,b)") -> string { return name; }`;
  assert.equal(findFunctions(source).length, 1);
  assert.deepEqual(findFunctions(source)[0].parameters, ['name: string = "a,b)"']);
  assert.equal(findContracts(source).length, 0);
});

test("module exports exclude provider methods and function locals", () => {
  const source = `contract Clock { func now() -> int; }\nprovider SystemClock for Clock { func now() -> int { return 0; } }\nfunc start() -> unit { let hidden = 1; }`;
  assert.deepEqual(topLevelDeclarations(source, findFunctions(source)).map(({ name }) => name), ["start"]);
  assert.deepEqual(topLevelDeclarations(source, findVariables(source)), []);
});

test("literal inference agrees with signed boundaries, uint64, and Orni escapes", () => {
  assert.equal(inferIntegerType("-128"), "int8");
  assert.equal(inferIntegerType("9223372036854775808"), "uint64");
  assert.equal(inferIntegerType("18446744073709551616"), undefined);
  assert.equal(findVariables('let text = "a\\nb";')[0].inferredType, "string<3>");
  assert.equal(findVariables('let text = "a\\qb";')[0].inferredType, undefined);
  assert.equal(findVariables('let initial = \'😀\';')[0].inferredType, "char");
});
