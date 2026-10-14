const assert = require("node:assert/strict");
const test = require("node:test");
const { diagnosticGuidance } = require("../diagnostic-guidance");
const { diagnosticCatalog } = require("../diagnostic-catalog");
const fs = require("node:fs");
const path = require("node:path");

test("every specific compiler error identity has editor guidance", () => {
  const sourceRoot = path.resolve(__dirname, "../../../src");
  const codes = new Set();
  for (const file of fs.readdirSync(sourceRoot, { recursive: true }).filter((file) => file.endsWith(".rs"))) {
    for (const [code] of fs.readFileSync(path.join(sourceRoot, file), "utf8").matchAll(/ORNI-E\d{3}/g)) codes.add(code);
  }
  for (const code of codes) assert(diagnosticCatalog[code], `Missing guidance for ${code}`);
});

for (const code of Object.keys(diagnosticCatalog)) {
  test(`stable guidance survives changed or misleading message wording: ${code}`, () => {
    const expected = diagnosticGuidance({ code });
    assert.deepEqual(diagnosticGuidance({ code, message: "Completely rewritten message" }), expected);
    assert.deepEqual(diagnosticGuidance({ code: { value: code }, message: "division by zero" }), expected);
    assert(expected.steps.length >= 2);
  });
}

const cases = [
  ["circular provider dependency detected: Audit -> Logger -> Audit", "Break the provider dependency cycle", /third independent contract/],
  ["provider dependency chain is incomplete: Audit -> Clock; contract 'Clock' has no provider binding", "Configure a provider for Clock", /bind Clock to YourProvider/],
  ["undefined variable 'missing_value'", "Declare or import missing_value", /let before/],
  ["undefined function 'greet'", "Declare or import greet", /func greet/],
  ["value 256 is out of range for uint8", "Keep the value inside the uint8 range", /0 to 255/],
  ["value -129 is out of range for int8", "Keep the value inside the int8 range", /-128 to 127/],
  ["cannot negate an unsigned integer; convert it to a signed type first", "Use a signed type before negation", /signed type/],
  ["variable value expects string<4>, but found string<5>", "Adjust the string capacity", /5 UTF-8 bytes/],
  ["variable value expects bool, but found string<3>", "Make the value match the declared type", /true or false/],
  ["function 'greet' expects between 1 and 2 argument(s), but received 3", "Adjust the arguments to greet", /supplies 3/],
  ["provider 'EmptyLogger' is missing contract method 'log'", "Implement log in EmptyLogger", /copy the log method signature/],
  ["provider method 'log' does not match contract 'Logger'", "Match the Logger contract signature", /number, order, and types/],
  ["contract methods cannot declare default values; pass arguments explicitly", "Remove the contract parameter default", /Pass that argument explicitly/],
  ["provider dependencies cannot declare default values", "Use a binding instead of a dependency default", /Remove the default/],
  ["provider dependency 'prefix' must use a contract type", "Declare an injectable contract dependency", /primitive dependency parameter/],
  ["unknown contract type 'Logger'", "Declare or import Logger", /current module/],
  ["provider 'Clock' implements 'Clock', not 'Logger'", "Bind a provider implementing the requested contract", /for clause/],
  ["contract 'Clock' is already bound", "Keep one binding per contract", /Remove the duplicate/],
  ["parameter name is already declared", "Remove or rename the conflicting declaration", /distinct name/],
  ["function 'greet' must return a string value on every path", "Return a compatible value on every path", /final return/],
  ["return can only be used inside a function", "Move return into a function", /top level/],
  ["contract 'Logger' has no method 'write'", "Use a method declared by Logger", /spelling of write/],
  ["circular import detected: first.orni -> second.orni -> first.orni", "Break the circular module imports", /reverse import/],
  ["could not read module 'contracts/audit.orni': No such file", "Correct the module file or import path", /including case/],
  ["module import is ambiguous between 'a.orni' and 'b.orni'", "Make the module import unambiguous", /Rename or move/],
  ["module 'a.orni' is already imported as 'a'", "Use one canonical module import", /same project source root/],
  ["private member '_helper' cannot be imported directly", "Use the module's public API", /public wrapper/],
  ["module 'a.orni' has no public member named 'greet'", "Import an existing public declaration", /not automatically re-exported/],
  ["imported modules may only contain imports, contracts, providers, functions, and module variables", "Move application setup into the entry file", /bind statements/],
  ["imports require a file-based check or run", "Check the program as a file", /Save the source/],
  ["selective imports cannot also declare a namespace alias", "Choose a selective import or a namespace alias", /Remove as alias/],
  ["module 'a.orni' resolves outside the Orni project root", "Keep source paths inside the project", /symlinks or junctions/],
  ["unknown runtime setting 'max_stuff'", "Correct the orni.toml project settings", /positive integer values/],
  ["division by zero", "Guard the divisor before dividing", /nonzero branch/],
  ["integer arithmetic overflow", "Prevent integer arithmetic overflow", /Widening only the result/],
  ["value 9223372036854775807 cannot be represented exactly as double", "Keep large exact values as integers", /precision/],
  ["function call depth exceeds the configured limit of 128", "Add a terminating recursion condition", /base case/],
  ["retained strings exceed the configured limit of 3 UTF-8 byte(s)", "Reduce retained string data", /smaller lexical scopes/],
  ["output exceeds the configured limit of 3 UTF-8 byte(s)", "Reduce captured output", /fewer or shorter/],
  ["string requires 20 UTF-8 byte(s), but the runtime limit is 10", "Bound string growth before allocation", /concatenations/],
  ["variable count exceeds the configured limit of 2", "Reduce simultaneously active bindings", /scopes end earlier/],
  ["execution exceeds the configured limit of 100 operation(s)", "Reduce work or terminate the computation", /termination/],
  ["string inference exceeds the limit of 64 calls", "Simplify the inference workload", /explicit string<N>/],
  ["source exceeds the limit of 1 MiB", "Reduce the compilation input", /token count/],
  ["expression tree exceeds the limit of 64 levels", "Flatten the expression or nested blocks", /intermediate let values/],
  ["unterminated string", "Close the string literal", /closing double quote/],
  ["a character literal must contain exactly one character", "Use exactly one character", /Use string/],
  ["unsupported string escape '\\q'", "Use a supported escape sequence", /unintended backslash/],
  ["unexpected character '@'", "Remove or replace the unexpected character", /unsupported punctuation/],
  ["required parameters cannot follow optional parameters", "Place required parameters first", /without defaults/],
  ["string capacity must be a positive platform-sized integer", "Use a positive string byte capacity", /string<32>/],
  ["'pub' is unnecessary; module declarations are public unless their name starts with '_'", "Remove pub from the declaration", /public by default/],
  ["only named functions can be called", "Call a named function or provider method", /First-class|first-class/],
  ["expected ';' after the declaration", "Add or correct ';'", /Insert or correct/],
  ["expected an expression", "Complete the declaration or expression", /Supply the missing/],
];

for (const [message, title, detail] of cases) {
  test(`guides: ${message}`, () => {
    const result = diagnosticGuidance({ message, code: "ORNI-T001" });
    assert.equal(result.title, title);
    assert.match(result.steps.join(" "), detail);
    assert(!result.steps.some((step) => /No automatic fix is available|Read the Problems message/.test(step)));
  });
}

for (const code of ["ORNI-W001", "ORNI-W002", "ORNI-W003", "ORNI-W004", "ORNI-L001", "ORNI-M001", "ORNI-S001", "ORNI-R001", "orni.unusedVariable", "orni.compilerUnavailable"]) {
  test(`provides code-specific guidance for ${code}`, () => {
    const result = diagnosticGuidance({ message: "Diagnostic details", code });
    assert(result.steps.length >= 2);
    assert.notEqual(result.title, "Correct the declaration or value at this location");
  });
}

test("unknown diagnostics retain useful guidance without pretending to offer an automatic fix", () => {
  assert(diagnosticGuidance({ message: "A future error", code: "ORNI-T999" }).steps.length >= 2);
});
