/** Stable error identities. Message wording is deliberately not part of this contract. */
const diagnosticCatalog = {
  "ORNI-E101": ["Break the provider dependency cycle", "Remove or replace one dependency in the provider parameters and update the methods that use it. Alternatively, move shared behavior into a third independent contract.", "Adding another bind cannot break a cycle. Choose a dependency direction that does not lead back to the same provider."],
  "ORNI-E102": ["Configure the missing provider binding", "Declare or import a provider implementing the missing contract, then add bind Contract to Provider; in the application entry file.", "Bind the provider's own dependencies too. Use the binding or provider-creation Quick Fix when offered; imported modules must not contain bind statements."],
  "ORNI-E103": ["Declare or import the missing name", "Check the spelling and scope. Declare a variable with let before using it, or declare the function with func.", "Import existing public module members using a selective import or a module namespace. Local variables are not visible outside their scope; names starting with _ are private."],
  "ORNI-E104": ["Keep the value inside the integer range", "Reduce or validate the value before assigning, returning, or passing it. Select a signed or unsigned integer type with a sufficient range.", "Widen arithmetic operands before the operation; widening only the result declaration cannot prevent an earlier overflow."],
  "ORNI-E105": ["Use a signed type before negation", "Copy the value into a signed type that can contain it, then negate the signed value.", "Check that the signed type's maximum is not smaller than the original unsigned value."],
  "ORNI-E106": ["Adjust the string capacity", "Use a shorter value, increase the explicit string<N> bound when the API permits it, or use plain string for automatic inference.", "Capacity is measured in UTF-8 bytes, not characters. For function arguments, change the parameter contract only if the function should accept larger input."],
  "ORNI-E107": ["Make the value match the declared type", "Change the expression to produce the expected type, or change its declaration if the current value is intentional.", "Update callers and contract implementations consistently. Use true or false for bool values and do not rely on implicit narrowing or string-to-number conversions."],
  "ORNI-E108": ["Adjust the function arguments", "Compare the call with the declaration. Add missing arguments or remove extra ones, preserving parameter order.", "Contract parameters are injected, not passed explicitly. Parameters with defaults may be omitted."],
  "ORNI-E109": ["Implement the missing contract method", "Copy the required method signature from the contract into the provider and implement its body.", "Match parameter types and return type, returning a compatible value on every path for non-unit methods."],
  "ORNI-E110": ["Match the contract method signature", "Compare the provider method with its contract: parameter count, order, types, and return type must match.", "Change the contract only if all providers and callers should adopt the new API."],
  "ORNI-E111": ["Remove the unsupported default value", "Remove the = value portion from the contract method or provider dependency parameter.", "Pass contract method arguments explicitly. Configure provider dependencies through contract bindings in the entry file."],
  "ORNI-E112": ["Declare an injectable contract dependency", "Replace the primitive provider dependency type with a declared contract type.", "Implement and bind that contract. Pass ordinary string or integer values to methods instead of declaring them as provider dependencies."],
  "ORNI-E113": ["Declare or import the contract or provider", "Check the declaration name and imports; the name must resolve in the current module.", "Use the contract on the left of bind and a compatible provider on the right."],
  "ORNI-E114": ["Bind a compatible provider", "Compare the bind contract with the provider's for clause, including the module identity.", "Select a provider that implements the requested contract, not an unrelated implementation."],
  "ORNI-E115": ["Keep one binding per contract", "Remove the duplicate bind statement from the entry file.", "Select the one provider this application should inject. Separate applications may select different providers."],
  "ORNI-E116": ["Remove or rename the conflicting declaration", "Find the other declaration, import, parameter, or namespace with the same name in this scope.", "Remove the duplicate or choose a distinct name and update references. Use namespace imports to avoid collisions."],
  "ORNI-E117": ["Return a compatible value on every path", "Add returns to every branch or add a final return after the conditional.", "Use -> unit only if the function should not produce a value; update callers accordingly."],
  "ORNI-E118": ["Move return into a function", "Use return inside a func body to produce a result.", "At the top level, remove return and use print or a function call instead."],
  "ORNI-E119": ["Use a method declared by the contract", "Check the method spelling against the injected dependency's contract.", "Use an existing method, or add the intended method to the contract and implement it in every provider."],
  "ORNI-E120": ["Break the circular module imports", "Remove the reverse import that closes the cycle.", "Move shared declarations into an independent module that both modules can import."],
  "ORNI-E121": ["Correct the module file or import path", "Verify that the imported .orni file exists and that the path and file name match, including case.", "Check paths relative to the importer and project.source. Keep modules inside the project root."],
  "ORNI-E122": ["Make the module import unambiguous", "Rename or move one of the competing modules so the import resolves to a single file.", "Avoid having both a relative module and a project-root module with the same import path."],
  "ORNI-E123": ["Use one canonical module import", "Import the module consistently through the same project source root.", "Remove competing aliases or paths that load the same file with different module identities."],
  "ORNI-E124": ["Use the module's public API", "Use a public declaration or add a public wrapper inside the owning module.", "Names starting with _ are private. Remove the prefix only if the declaration should become public API."],
  "ORNI-E125": ["Import an existing public declaration", "Check spelling and inspect the module's top-level public declarations.", "Import from the module that declares the member; imported members are not automatically re-exported."],
  "ORNI-E126": ["Move application setup into the entry file", "Move print, executable calls, and bind statements into the application's entry file.", "Keep imports, contracts, providers, functions, and module variables in reusable modules."],
  "ORNI-E127": ["Check the program as a file", "Save the source as an .orni file and check or run that file instead of a source-only snippet.", "Use an orni.toml entry when imports belong to a project."],
  "ORNI-E128": ["Choose a selective import or a namespace alias", "Remove as alias from the selective import, or import the entire module with an alias.", "Use selected names directly or qualify calls through the namespace."],
  "ORNI-E129": ["Keep source paths inside the project", "Place imported files and configured source paths under the directory containing orni.toml.", "Remove parent-directory traversal and check symlinks or junctions that resolve outside the project root."],
  "ORNI-E130": ["Correct the orni.toml project settings", "Use supported [project] and [runtime] settings and valid TOML syntax. source must be an existing directory and entry an .orni file.", "Remove unknown settings and use positive integers for supported runtime limits."],
  "ORNI-E131": ["Guard the divisor before dividing", "Check whether the divisor equals zero before evaluating the division.", "Reject the input or return an appropriate result in that branch; divide only in the nonzero branch."],
  "ORNI-E132": ["Prevent integer arithmetic overflow", "Validate operands and choose a sufficiently wide integer type before performing the operation.", "Widening only the result declaration is too late. Declare operands as int when inferred int8 operands cannot hold the result."],
  "ORNI-E133": ["Keep large exact values as integers", "Keep the calculation in an integer type when exact precision is required.", "Reduce the value before mixing it with double operands or choose a representation that tolerates rounding."],
  "ORNI-E134": ["Add a terminating recursion condition", "Add a base case and ensure each recursive call moves toward it.", "Reduce recursion depth. Increase max_call_depth only after checking the host stack and a finite recursion bound."],
  "ORNI-E135": ["Reduce retained string data", "Keep large strings in smaller lexical scopes and avoid retaining unnecessary intermediate values.", "Reduce input size; increase max_retained_string_bytes only within a measured memory budget."],
  "ORNI-E136": ["Reduce captured output", "Print fewer or shorter messages instead of accumulating a complete large payload.", "Increase max_output_bytes only when expected output fits the host memory budget."],
  "ORNI-E137": ["Bound string growth before allocation", "Reduce repeated concatenation and validate UTF-8 input sizes before producing large strings.", "Respect explicit string<N> contracts; max_string_bytes cannot override an explicit bound."],
  "ORNI-E138": ["Reduce simultaneously active bindings", "Move temporary values into smaller scopes so bindings are released earlier.", "Check recursive calls and increase max_variables only within a measured resource budget."],
  "ORNI-E139": ["Reduce work or terminate the computation", "Check recursive termination and avoid repeated computation.", "Reduce input size or improve the algorithm; increase max_operations only for finite, measured work."],
  "ORNI-E140": ["Simplify the inference workload", "Split deeply chained calls and avoid repeatedly expanding the same string-return expression.", "Use explicit string<N> return contracts when a fixed bound is known. Runtime limits do not raise compiler limits."],
  "ORNI-E141": ["Reduce the compilation input", "Reduce source size, token count, module count, or import depth according to the reported limit.", "Remove unused imports or split independent applications. Runtime settings do not raise compiler safeguards."],
  "ORNI-E142": ["Flatten the expression or nested blocks", "Split expressions into intermediate let values or smaller functions and reduce nested blocks.", "Preserve evaluation order when breaking up chained operators or nested expressions."],
  "ORNI-E143": ["Close the string literal", "Add the closing double quote and escape embedded quotes.", "Check whether a trailing backslash unintentionally escapes the closing quote."],
  "ORNI-E144": ["Use exactly one character", "Put one Unicode scalar value between single quotes, closing the literal.", "Use string for multiple characters and supported escapes for special characters."],
  "ORNI-E145": ["Use a supported escape sequence", "Use supported escapes such as \\n, \\t, escaped quotes, or an escaped backslash.", "Remove unintended backslashes. Literal Unicode characters may be written directly."],
  "ORNI-E146": ["Remove or replace the unexpected character", "Replace unsupported punctuation or typographic quotes with valid Orni syntax.", "Check pasted invisible characters and use // for comments."],
  "ORNI-E147": ["Place required parameters first", "Move parameters without defaults before parameters with defaults.", "Update positional calls to follow the new order."],
  "ORNI-E148": ["Use a positive string byte capacity", "Use a positive platform-sized integer such as string<32>.", "Use plain string for automatic inference rather than an invalid or zero capacity."],
  "ORNI-E149": ["Remove pub from the declaration", "Declarations are public by default; remove pub.", "Prefix the name with _ when the declaration should be private."],
  "ORNI-E150": ["Call a named function or provider method", "Use a declared func name or an injected dependency method as the call target.", "First-class function values and calls on arbitrary expression results are not supported."],
  "ORNI-E151": ["Use a supported member-access target", "Access a member through an imported module namespace or a named injected dependency.", "Arbitrary object property access is not supported in this preview."],
};

const codeExamples = {
  "ORNI-E102": "bind Clock to SystemClock;",
  "ORNI-E103": 'let project = "Orni";\nprint project;',
  "ORNI-E105": "let port: uint16 = 80;\nlet signed_port: int = port;\nprint -signed_port;",
  "ORNI-E106": 'let name: string = "Lucas";\n// Optional explicit bound:\nlet bounded_name: string<5> = "Lucas";',
  "ORNI-E107": "let enabled: bool = true;",
  "ORNI-E131": 'if divisor == 0 {\n    print "Cannot divide by zero";\n} else {\n    print total / divisor;\n}',
  "ORNI-E147": 'func greet(name: string, greeting: string = "Hello") -> string {\n    return greeting + ", " + name;\n}',
};

/** Returns a fresh explanation so callers cannot mutate the shared catalog. */
function guidanceForCode(code) {
  const entry = diagnosticCatalog[code];
  return entry && { title: entry[0], steps: entry.slice(1), ...(codeExamples[code] ? { example: codeExamples[code] } : {}) };
}

module.exports = { diagnosticCatalog, guidanceForCode };
