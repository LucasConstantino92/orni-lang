const { guidanceForCode } = require("./diagnostic-catalog");

/** Creates a consistent explanation shared by Quick Fix, hover, and manual help. */
function guidance(title, steps, example) {
  return { title, steps, ...(example ? { example } : {}) };
}

/** Returns actionable guidance for an Orni diagnostic without changing its code. */
function diagnosticGuidance(diagnostic = {}) {
  const message = diagnostic.message || "";
  const code = diagnostic.code && typeof diagnostic.code === "object" ? diagnostic.code.value : diagnostic.code;
  const stableGuidance = guidanceForCode(code);
  if (stableGuidance) return stableGuidance;
  // Older configured compilers only report phase-wide codes. Keep their richer
  // explanations until users upgrade; current compilers use stable identities.
  let match;

  if ((match = message.match(/circular provider dependency detected: (.+)/))) {
    return guidance("Break the provider dependency cycle", [
      `The cycle is ${match[1]}. Each provider eventually depends on itself.`,
      "Remove or replace one dependency in the provider parameters and update the methods that use it. Alternatively, move the shared behavior into a third independent contract.",
      "Adding another bind cannot break a cycle. Choose the dependency direction that matches your application design.",
    ]);
  }
  if ((match = message.match(/contract '([^']+)' has no provider binding/))) {
    const name = match[1].split(".").at(-1);
    return guidance(`Configure a provider for ${name}`, [
      `Declare or import a provider implementing ${name}.`,
      `Add bind ${name} to YourProvider; in the application entry file. Bind the provider's own dependencies too.`,
      "Use the binding or provider-creation Quick Fix when it is offered. Imported modules must not contain bind statements.",
    ], `bind ${name} to YourProvider;`);
  }
  if ((match = message.match(/undefined (variable|function) '([^']+)'/))) {
    const [, kind, name] = match;
    return guidance(`Declare or import ${name}`, [
      `Check the spelling and scope of ${kind} '${name}'.`,
      kind === "variable" ? `Declare ${name} with let before using it. A local variable from another function or block is not visible here.` : `Declare func ${name.split(".").at(-1)} with the required parameters and return type, or import its module.`,
      "For an existing public module member, use a selective import or its imported namespace. Names starting with _ are private.",
    ]);
  }
  if ((match = message.match(/(?:out of range for|exceeds) (u?int(?:8|16|32|64)?)(?: range)?/))) {
    const type = match[1];
    const bits = BigInt(type.match(/\d+/)?.[0] || 64);
    const unsigned = type.startsWith("u");
    const minimum = unsigned ? 0n : -(1n << (bits - 1n));
    const maximum = unsigned ? (1n << bits) - 1n : (1n << (bits - 1n)) - 1n;
    return guidance(`Keep the value inside the ${type} range`, [
      `${type} accepts values from ${minimum} to ${maximum}. Reduce or validate the value before assigning, returning, or passing it.`,
      "For arithmetic, widen the operands before performing the operation. Widening only the result declaration cannot prevent an earlier overflow.",
      bits < 64n ? "If the larger value is valid for your application, choose a wider integer type in the declaration and any affected contracts." : "This is already a 64-bit range. Values outside the supported ranges need a different representation, such as text, or must be rejected.",
    ]);
  }
  if (/cannot negate an unsigned/.test(message)) {
    return guidance("Use a signed type before negation", ["Copy the value into an int or another signed type that can contain it, then negate the signed value.", "Do not use a signed type whose maximum is smaller than the original unsigned value."], "let port: uint16 = 80;\nlet signed_port: int = port;\nprint -signed_port;");
  }
  if ((match = message.match(/(?:expects string<(\d+)>, but found string<(\d+)>|requires (\d+) UTF-8 bytes, but string<(\d+)>)/))) {
    const maximum = match[1] || match[4];
    const actual = match[2] || match[3];
    return guidance("Adjust the string capacity", [
      `The value requires ${actual} UTF-8 bytes, but the declaration allows ${maximum}. Use a shorter value or increase the explicit capacity when the API permits it.`,
      `Use plain string for automatic inference, or string<${actual}> for an explicit bound. Accented characters can occupy multiple UTF-8 bytes.`,
      "For a function argument, update the parameter contract only if the function is intended to accept the larger input.",
    ]);
  }
  if ((match = message.match(/(.+) expects (.+), but found (.+)/))) {
    return guidance("Make the value match the declared type", [
      `The ${match[1]} must be ${match[2]}; the current expression is ${match[3]}.`,
      "Change the expression to produce the expected type, or change the declaration if the current value is intentional. Update callers and contract implementations consistently.",
      /bool/.test(match[2]) ? 'Use true or false for bool values; text such as "yes" is a string.' : "Do not rely on an implicit narrowing or a string-to-number conversion.",
    ]);
  }
  if ((match = message.match(/function '([^']+)' expects (.+) argument\(s\), but received (\d+)/))) {
    return guidance(`Adjust the arguments to ${match[1]}`, [
      `This call requires ${match[2]} explicit arguments; it supplies ${match[3]}. Add missing arguments or remove the extra ones.`,
      "Follow the declaration's parameter order. Parameters typed as contracts are injected; do not pass them explicitly. Optional defaults may be omitted.",
    ]);
  }
  if ((match = message.match(/provider '([^']+)' is missing contract method '([^']+)'/))) {
    return guidance(`Implement ${match[2]} in ${match[1]}`, [
      `Open the implemented contract and copy the ${match[2]} method signature into provider ${match[1]}.`,
      "Implement the method body with matching parameter types and return type. Return a compatible value on every path for non-unit methods.",
    ]);
  }
  if ((match = message.match(/provider method '([^']+)' does not match contract '([^']+)'/))) {
    return guidance(`Match the ${match[2]} contract signature`, [
      `Compare provider method ${match[1]} with contract ${match[2]}. Match the number, order, and types of parameters and the return type.`,
      "Keep implementation logic inside the provider body. Change the contract only if all its providers and callers should adopt the new API.",
    ]);
  }
  if (/contract methods cannot declare default/.test(message)) {
    return guidance("Remove the contract parameter default", ["Remove the = value portion from the contract method parameter.", "Pass that argument explicitly at each call. Contract method defaults are not supported in this preview."], "contract Logger {\n    func log(message: string) -> unit;\n}");
  }
  if (/provider dependencies cannot declare default/.test(message)) {
    return guidance("Use a binding instead of a dependency default", ["Remove the default value from the provider dependency parameter.", "Use a contract type for the dependency and configure its provider with bind in the application entry file."]);
  }
  if (/provider dependency .+ must use a contract type/.test(message)) {
    return guidance("Declare an injectable contract dependency", ["Replace the primitive dependency parameter type with a declared contract type.", "Implement that contract in a provider and bind it in the entry file. Pass ordinary string or integer values to methods instead of declaring them as provider dependencies."]);
  }
  if ((match = message.match(/(?:unknown contract(?: type)?|unknown provider) '([^']+)'/))) {
    return guidance(`Declare or import ${match[1]}`, ["Check the declaration name and its import. Contract and provider names must resolve in the current module.", "Declare the contract before implementing its behavior, or import the existing public declaration. Use the provider name on the right side of bind."]);
  }
  if (/implements '.+', not '.+'/.test(message)) {
    return guidance("Bind a provider implementing the requested contract", ["Compare the bind contract with the provider's for clause. They must identify the same contract, including its module.", "Select a compatible provider instead of binding an unrelated implementation."]);
  }
  if (/already bound/.test(message)) {
    return guidance("Keep one binding per contract", ["Remove the duplicate bind statement from the application entry.", "Choose the one provider this application should inject for the contract. Different applications may select different implementations."]);
  }
  if (/already declared|conflicts with a declaration/.test(message)) {
    return guidance("Remove or rename the conflicting declaration", ["Find the other declaration, imported member, parameter, or namespace with the same name in this scope.", "Remove the duplicate or give one declaration a distinct name, then update its references. Use an import namespace when selective imports collide."]);
  }
  if (/must return a .+ value on every path/.test(message)) {
    return guidance("Return a compatible value on every path", ["Add a return statement to every possible branch, or add a final return after the conditional.", "Use -> unit only if the function should not produce a value; update its callers accordingly."]);
  }
  if (/return can only be used inside a function/.test(message)) {
    return guidance("Move return into a function", ["Use return inside a func body to produce a result.", "At the top level, use print to display a value or call a function; remove the top-level return keyword."]);
  }
  if ((match = message.match(/contract '([^']+)' has no method '([^']+)'/))) {
    return guidance(`Use a method declared by ${match[1]}`, [`Check the spelling of ${match[2]} and open contract ${match[1]} to inspect its supported methods.`, "If this operation belongs in the contract, add its signature and implement it in every provider. Provider-only methods are not exposed through a contract parameter."]);
  }
  if (/circular import detected/.test(message)) {
    return guidance("Break the circular module imports", ["Follow the import chain in the diagnostic and remove one reverse import.", "Move shared contracts or helpers into an independent module that both original modules can import."]);
  }
  if (/could not read module/.test(message)) {
    return guidance("Correct the module file or import path", ["Check that the reported .orni file exists and that its file name matches the dotted import path, including case.", "Resolve the path relative to the importing file or project.source in orni.toml. Use the suggested module name if the compiler provides one."]);
  }
  if (/module import is ambiguous/.test(message)) {
    return guidance("Make the module import unambiguous", ["The same dotted import resolves to two different files: one beside the importing module and one under project.source.", "Rename or move one module, or use a dotted path that identifies only one of the reported files."]);
  }
  if (/already imported as/.test(message)) {
    return guidance("Use one canonical module import", ["Use the same project source root and module path everywhere this file is imported.", "Avoid reaching one file through different relative/root paths or namespaces. Consolidate duplicate imports in the importing module."]);
  }
  if (/private member|private members/.test(message)) {
    return guidance("Use the module's public API", ["Names starting with _ are private to their declaring module.", "Call a public wrapper, or intentionally rename the declaration without _ and update its references if it should become part of the public API."]);
  }
  if (/has no public member named/.test(message)) {
    return guidance("Import an existing public declaration", ["Open the reported module and verify the selected member's spelling and visibility.", "Import the module that directly declares the member. Imported members are not automatically re-exported by another module."]);
  }
  if (/imported modules may only contain/.test(message)) {
    return guidance("Move application setup into the entry file", ["Move top-level print statements, calls, conditionals, and bind statements into the application entry file.", "Keep only imports, contracts, providers, functions, and immutable module values in a reusable module."]);
  }
  if (/imports require a file-based/.test(message)) {
    return guidance("Check the program as a file", ["Save the source as an .orni file and use orni check <file.orni> or orni run <file.orni>.", "Declare project.source in orni.toml when imports should resolve from an application root."]);
  }
  if (/selective imports cannot also/.test(message)) {
    return guidance("Choose a selective import or a namespace alias", ["Remove as alias from the selective import, or import the entire module with an alias.", "Use the selected names directly, or qualify each call through the chosen namespace."], "import services.greeting.{greet};\n// Alternative: import services.greeting as welcome;");
  }
  if (/outside .*project|stay inside the project/.test(message)) {
    return guidance("Keep source paths inside the project", ["Move the imported file or configured source directory under the directory containing orni.toml.", "Remove parent-directory traversal and check symlinks or junctions that may resolve outside the project root."]);
  }
  if (/manifest|project\.(?:name|entry|source)|runtime must be a table|runtime\.[A-Za-z_]+|unknown runtime setting/.test(message)) {
    return guidance("Correct the orni.toml project settings", ["Use a [project] table with name, entry, and source. source must name an existing directory and entry an .orni file inside the project.", "Use supported limit names inside [runtime] and positive integer values. Remove misspelled or unsupported keys; check the TOML syntax at the reported location."], '[project]\nname = "application"\nentry = "main.orni"\nsource = "."\n\n[runtime]\nmax_operations = 100000');
  }
  if (/division by zero/.test(message)) {
    return guidance("Guard the divisor before dividing", ["Check whether the divisor equals zero before evaluating the division.", "Return an appropriate result or reject the input in that branch. Only perform the division in the nonzero branch."], "if divisor == 0 {\n    print \"Cannot divide by zero\";\n} else {\n    print total / divisor;\n}");
  }
  if (/integer arithmetic overflow/.test(message)) {
    return guidance("Prevent integer arithmetic overflow", ["Validate the operands and choose a sufficiently wide integer type before performing the operation.", "Widening only the result declaration is too late: the operation may overflow first. For inferred int8 values, declare the operands as int when a larger result is expected."]);
  }
  if (/cannot be represented exactly as double/.test(message)) {
    return guidance("Keep large exact values as integers", ["Keep this calculation in an integer type if exact precision is required.", "Reduce the value before mixing it with double operands, or explicitly choose a representation that tolerates rounding. Orni does not silently discard integer precision."]);
  }
  if (/function call depth|recursion/.test(message)) {
    return guidance("Add a terminating recursion condition", ["Add a base case and ensure each recursive call moves toward it.", "Reduce the recursion depth or restructure the algorithm. Increase max_call_depth in [runtime] only after confirming the recursion is bounded and the host has sufficient stack capacity."]);
  }
  if (/retained strings/.test(message)) {
    return guidance("Reduce retained string data", ["Keep large strings in smaller lexical scopes and avoid retaining unnecessary intermediate values across calls.", "Reduce the input size. Change max_retained_string_bytes in [runtime] only when the application's measured memory budget permits it."]);
  }
  if (/output (?:exceeds|size overflow)/.test(message)) {
    return guidance("Reduce captured output", ["Print fewer or shorter messages instead of accumulating the entire payload in program output.", "Use a finite output budget. Increase max_output_bytes in [runtime] only if the expected output fits the host memory budget."]);
  }
  if (/string requires|(?:retained )?string size overflow/.test(message)) {
    return guidance("Bound string growth before allocation", ["Check the UTF-8 size of inputs and intermediate concatenations; reduce repeated or recursive string growth.", "Respect explicit string<N> contracts. Change max_string_bytes in [runtime] only for a bounded, measured workload; it does not override an explicit string<N> limit."]);
  }
  if (/variable count exceeds/.test(message)) {
    return guidance("Reduce simultaneously active bindings", ["Move temporary values into smaller blocks or functions so their scopes end earlier.", "Check recursive calls that keep many parameters alive. Increase max_variables only when the additional retained data fits the resource budget."]);
  }
  if (/execution exceeds/.test(message)) {
    return guidance("Reduce work or terminate the computation", ["Check recursive termination conditions and reduce repeated computations.", "Use smaller input or a more efficient algorithm. Increase max_operations only when the work is finite and the application has a measured execution budget."]);
  }
  if (/semantic analysis exceeds|string inference exceeds/.test(message)) {
    return guidance("Simplify the inference workload", ["Split deeply chained function calls into smaller independent operations and avoid repeatedly expanding the same string-return expression.", "Consider explicit string<N> return contracts where a fixed bound is known. Runtime limits do not change compiler analysis limits."]);
  }
  if (/module graph exceeds|source exceeds/.test(message)) {
    return guidance("Reduce the compilation input", ["Reduce generated source size, token count, module count, or import depth according to the limit in the diagnostic.", "Remove unused imports and split the application into smaller independent entry points where appropriate. Runtime settings cannot raise compiler safeguards."]);
  }
  if (/syntax nesting|expression (?:tree|chain) exceeds/.test(message)) {
    return guidance("Flatten the expression or nested blocks", ["Split the expression into named intermediate let values or smaller functions.", "Reduce nested parentheses, blocks, or chained operators. Preserve the original order of evaluation when splitting the code."]);
  }
  if (/unterminated string/.test(message)) return guidance("Close the string literal", ['Add the closing double quote before the statement ends. Escape an embedded quote as \\".', "Check whether a trailing backslash accidentally escapes the closing quote."]);
  if (/unterminated character|character literal must contain/.test(message)) return guidance("Use exactly one character", ["Put one Unicode character between single quotes. Use string for multiple characters.", "Close the single quote and use only supported character escapes such as \\n or \\t."]);
  if (/unsupported .* escape/.test(message)) return guidance("Use a supported escape sequence", ['For strings, use \\n, \\t, \\" or \\\\. Remove an unintended backslash.', "Character literals also support an escaped single quote. Literal Unicode characters can be written directly."]);
  if (/unexpected character/.test(message)) return guidance("Remove or replace the unexpected character", ["Inspect the reported character and replace typographic quotes or unsupported punctuation with Orni syntax.", "Use // for comments and quote text with double quotes. Check for pasted invisible characters."]);
  if (/required parameters cannot follow optional/.test(message)) return guidance("Place required parameters first", ["Move parameters without defaults before parameters with defaults.", "Update positional calls to follow the new parameter order."]);
  if (/string capacity must be/.test(message)) return guidance("Use a positive string byte capacity", ["Replace the capacity with a positive platform-sized integer, such as string<32>.", "Use plain string to infer the capacity instead of supplying an invalid or zero bound."]);
  if (/'pub' is unnecessary/.test(message)) return guidance("Remove pub from the declaration", ["Orni declarations are public by default. Remove the pub keyword.", "Prefix the declaration name with _ if it should be private to its module."]);
  if (/only named functions can be called/.test(message)) return guidance("Call a named function or provider method", ["Use a declared func name or an injected dependency method as the call target.", "Do not call the result of another expression; first-class function values are not supported in this preview."]);
  if (/only module namespaces support member access/.test(message)) return guidance("Use a supported member-access target", ["Access a member through an imported module namespace or a named injected dependency.", "Store other expression results in a variable and use operations supported by their type. Arbitrary object property access is not supported."]);
  if ((match = message.match(/expected '([^']+)'/))) return guidance(`Add or correct '${match[1]}'`, [`${message[0].toUpperCase()}${message.slice(1)}. Insert or correct this token at the indicated location.`, "Also check the preceding expression or declaration: an earlier missing delimiter can shift where parsing fails."]);
  if (/expected (?:a |an )?/.test(message)) return guidance("Complete the declaration or expression", [`${message[0].toUpperCase()}${message.slice(1)}. Supply the missing identifier, literal, expression, or type at the reported position.`, "Check the token immediately before the error for a missing delimiter or an incomplete declaration."]);
  if (/if condition must be a boolean|'!' only accepts booleans/.test(message)) return guidance("Use a boolean condition", ["Use true, false, or a comparison expression instead of a number or text.", "The ! operator requires a bool value."]);
  if (/incompatible types|'\-' only accepts numbers/.test(message)) return guidance("Use operands supported by the operator", ["Use numeric operands for arithmetic and ordering, or string operands for text concatenation.", "Check each operand's hover type and correct the expression before changing its result declaration."]);
  if (code === "ORNI-W001" || code === "ORNI-W002") return guidance("Remove or use the unused import", ["Remove the unused module or selected member with the Quick Fix, or reference the imported API where it is needed.", "Keep imports that are part of the program's actual dependencies; unused imports still add compilation work."]);
  if (code === "ORNI-W003") return guidance("Remove or call the unused private function", ["Delete the helper if it is no longer needed, or call it from the module where its behavior is required.", "Remove the _ prefix only if the function intentionally belongs to the module's public API."]);
  if (code === "ORNI-W004") return guidance("Remove or inject the unused binding", ["Remove the bind statement with the Quick Fix, or request its contract through a function or provider dependency.", "Do not add an artificial use just to silence the warning; keep only bindings the application needs."]);
  if (code === "orni.unusedVariable") return guidance("Remove or intentionally mark the unused variable", ["Use the variable, remove its declaration if evaluating its initializer is unnecessary, or prefix its name with _ to document intentional non-use.", "The underscore Quick Fix preserves the initializer and changes only the declaration name."]);
  if (code === "orni.compilerUnavailable") return guidance("Restore the compiler connection", ["Reload the editor window after installing the latest Orni VSIX; it includes the default compiler.", "If orni.compilerPath is configured, verify that it points to a working Orni executable. For a timeout, simplify the source and run Orni: Analyze Workspace again."]);
  if (code === "ORNI-L001") return guidance("Correct the literal or source character", ["Check the highlighted literal, quoting, supported escapes, and numeric range.", "Replace the invalid token with a valid Orni literal before checking the surrounding expression."]);
  if (code === "ORNI-M001") return guidance("Correct the module or project declaration", ["Check the reported module path against project.source, file names, visibility, and declaration names.", "Keep executable setup in the entry file and reusable declarations in imported modules. Recheck the application after fixing the reported module."]);
  if (code === "ORNI-S001") return guidance("Complete valid Orni syntax", ["Check the highlighted declaration and its preceding delimiter. Balance parentheses and braces and terminate statements with a semicolon.", "Compare the statement with the matching example under examples before adding further code."]);
  if (code === "ORNI-R001") return guidance("Correct the failing runtime operation", ["Inspect the operation and its input values at the reported location. Validate them before executing that operation.", "For a configured resource boundary, reduce the work or retained data before considering a measured limit increase."]);
  return guidance("Correct the declaration or value at this location", ["Check the highlighted expression's inferred type, the declaration it refers to, and the relevant function or contract signature.", "Make the value and its declared API consistent. Recheck the file after correcting the first reported error, because later diagnostics may depend on it."]);
}

module.exports = { diagnosticGuidance };
