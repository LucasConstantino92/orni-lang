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
  assert: {
    signature: "assert condition;",
    summary: "Stops a test or program with an error when the boolean condition is false.",
    details: [
      "Use `assert` in a file under `tests` and run it with `orni test`.",
      "The condition must evaluate to `bool`.",
    ],
  },
  test: {
    signature: 'test "description" { ... }',
    summary: "Groups assertions into one named test scenario.",
    details: [
      "A test passes only when every statement in its body succeeds.",
      "Run files under `tests` with `orni test`; named scenarios are shown in the result.",
    ],
  },
};

const responseHelpers = {
  ok: ["response.ok(body)", "Returns a successful HTTP response with status 200."],
  created: ["response.created(body)", "Returns a successful HTTP response with status 201."],
  bad_request: ["response.bad_request(message)", "Returns a JSON API error with status 400."],
  unauthorized: ["response.unauthorized(message)", "Returns a JSON API error with status 401."],
  forbidden: ["response.forbidden(message)", "Returns a JSON API error with status 403."],
  not_found: ["response.not_found(message)", "Returns a JSON API error with status 404."],
  conflict: ["response.conflict(message)", "Returns a JSON API error with status 409."],
  internal_error: ["response.internal_error(message)", "Returns a JSON API error with status 500."],
  redirect: ["response.redirect(url)", "Returns a 302 redirect with the URL as its Location header."],
  no_content: ["response.no_content()", "Returns a successful HTTP response with status 204 and no body."],
  header: ["response.header(response, name, value)", "Sets or replaces one safe HTTP response header and returns the updated response."],
};

const responseHelperGuidance = {
  ok: ["Body", "Any value supported by the HTTP boundary. Strings become text; models and lists become JSON."],
  created: ["Body", "The created resource. Use this after a successful create operation."],
  bad_request: ["Message", "A client-safe explanation of why the request cannot be processed."],
  unauthorized: ["Message", "A client-safe explanation that authentication is required or invalid."],
  forbidden: ["Message", "A client-safe explanation that the caller lacks permission."],
  not_found: ["Message", "A client-safe explanation that the requested resource does not exist."],
  conflict: ["Message", "A client-safe explanation of a state conflict, such as a duplicate resource."],
  internal_error: ["Message", "A safe public message. Do not expose private implementation details."],
  redirect: ["URL", "An absolute or relative location sent as the HTTP Location header."],
  no_content: ["No parameters", "Returns status 204. HTTP clients must not expect a response body."],
  header: ["Response, name, value", "Returns a copy of response with one safe header added or replaced. Framing headers are protected."],
};

const apiHelpers = {
  status: ["api.status(method, path, body?)", "Calls an application route in memory and returns its HTTP status. `method` and `path` are required strings; `body` is optional JSON text for POST, PUT, and PATCH requests."],
  body: ["api.body(method, path, body?)", "Calls an application route in memory and returns its response body as text. `method` and `path` are required strings; `body` is optional JSON text."],
  contains: ["api.contains(method, path, fragment, body?)", "Calls an application route in memory and returns whether its response body contains the expected text fragment."],
};

const apiHelperGuidance = {
  status: ["Parameters", "method and path are required strings. body is optional JSON text for requests that accept a model body.", "Returns", "The numeric HTTP status returned by the route without opening a TCP listener."],
  body: ["Parameters", "method and path are required strings. body is optional JSON text for requests that accept a model body.", "Returns", "The response body as text. Use it to assert JSON or plain-text API output."],
  contains: ["Parameters", "method, path, and fragment are required strings. body is optional JSON text.", "Returns", "true when the response body includes the expected fragment, without opening a TCP listener."],
};

const storeHelpers = {
  list: [
    "store.list()\nstore.list(page: uint32, limit: uint16)\nstore.list(page: uint32, limit: uint16, sort: string, direction: string)",
    "Returns every stored model, or a bounded page with deterministic metadata.",
    [
      "Call without arguments for the complete JSON array.",
      "Call with `page` (starting at 1) and `limit` (1 through 100) to return `{ data, page, limit, total, total_pages }`.",
      "Pass a declared model field and `asc` or `desc` to sort before pagination.",
      "Use typed GET query parameters to pass pagination directly from the URL.",
    ],
  ],
};

const backendSymbols = {
  print: {
    path: ["src", "runtime", "interpreter.rs"],
    needle: "Statement::Print {",
  },
  assert: {
    path: ["src", "runtime", "interpreter.rs"],
    needle: "Statement::Assert {",
  },
  test: {
    path: ["src", "runtime", "interpreter.rs"],
    needle: "Statement::Test {",
  },
  app: {
    path: ["src", "runtime", "interpreter", "calls.rs"],
    needle: 'if name == "app.name" {',
  },
  api: {
    path: ["src", "runtime", "http.rs"],
    needle: "pub(crate) struct ApiTestContext",
  },
  func: {
    path: ["src", "runtime", "interpreter.rs"],
    needle: "Statement::Function {",
  },
  main: {
    path: ["src", "runtime", "interpreter.rs"],
    needle: "fn call_application_main(&mut self)",
  },
  model: {
    path: ["src", "frontend", "parser", "statements.rs"],
    needle: "fn model_statement(",
  },
  store: {
    path: ["src", "frontend", "parser", "statements.rs"],
    needle: "fn store_statement(",
  },
  route: {
    path: ["src", "frontend", "parser", "statements.rs"],
    needle: "fn route_statement(",
  },
  response: {
    path: ["src", "syntax", "ast.rs"],
    needle: "Response {",
  },
  "response.ok": {
    path: ["src", "runtime", "interpreter", "calls.rs"],
    needle: "if matches!(",
  },
  "response.created": {
    path: ["src", "runtime", "interpreter", "calls.rs"],
    needle: '"response.created" => 201,',
  },
  "response.not_found": {
    path: ["src", "runtime", "interpreter", "calls.rs"],
    needle: '"response.not_found" => 404,',
  },
  "response.bad_request": {
    path: ["src", "runtime", "interpreter", "calls.rs"],
    needle: '"response.bad_request" => 400,',
  },
  "response.unauthorized": {
    path: ["src", "runtime", "interpreter", "calls.rs"],
    needle: '"response.unauthorized" => 401,',
  },
  "response.forbidden": {
    path: ["src", "runtime", "interpreter", "calls.rs"],
    needle: '"response.forbidden" => 403,',
  },
  "response.conflict": {
    path: ["src", "runtime", "interpreter", "calls.rs"],
    needle: '"response.conflict" => 409,',
  },
  "response.internal_error": {
    path: ["src", "runtime", "interpreter", "calls.rs"],
    needle: '"response.internal_error" => 500,',
  },
  "response.redirect": {
    path: ["src", "runtime", "interpreter", "calls.rs"],
    needle: '"response.redirect" => 302,',
  },
  "response.no_content": {
    path: ["src", "runtime", "interpreter", "calls.rs"],
    needle: 'if name == "response.no_content" {',
  },
  "response.header": {
    path: ["src", "runtime", "interpreter", "calls.rs"],
    needle: 'if name == "response.header" {',
  },
  "api.status": {
    path: ["src", "runtime", "http.rs"],
    needle: "pub(crate) fn api_test_request",
  },
  "api.body": {
    path: ["src", "runtime", "http.rs"],
    needle: "pub(crate) fn api_test_request",
  },
  "api.contains": {
    path: ["src", "runtime", "interpreter", "calls.rs"],
    needle: '"api.status" | "api.body" | "api.contains"',
  },
  "store.create": {
    path: ["src", "runtime", "interpreter", "stores.rs"],
    needle: '"create" => {',
  },
  "store.list": {
    path: ["src", "runtime", "interpreter", "stores.rs"],
    needle: '"list" if values.is_empty() => Value::Response',
  },
  "store.find": {
    path: ["src", "runtime", "interpreter", "stores.rs"],
    needle: '"find" => match',
  },
  "store.replace": {
    path: ["src", "runtime", "interpreter", "stores.rs"],
    needle: '"replace" => {',
  },
  "store.patch": {
    path: ["src", "runtime", "interpreter", "stores.rs"],
    needle: '"patch" => {',
  },
  "store.delete": {
    path: ["src", "runtime", "interpreter", "stores.rs"],
    needle: '"delete" => match',
  },
};

for (const method of ["get", "post", "put", "patch", "delete"]) {
  backendSymbols[method] = {
    path: ["src", "runtime", "http.rs"],
    needle: `TokenKind::${method[0].toUpperCase()}${method.slice(1)} => "${method.toUpperCase()}",`,
  };
}

for (const typeName of [
  "string", "int", "int8", "int16", "int32", "int64",
  "uint8", "uint16", "uint32", "uint64", "double", "bool", "char", "uuid", "unit",
]) {
  backendSymbols[typeName] = {
    path: ["src", "syntax", "ast.rs"],
    needle: "pub(crate) enum TypeName {",
  };
}

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
  model: "Declares the typed fields required by application data.",
  store: "Declares an in-memory collection of one model for the local HTTP server.",
  provider: "Implements every method required by a dependency contract.",
  for: "Connects a provider declaration to the contract it implements.",
  bind: "Selects the provider that Orni injects for a contract.",
  to: "Connects a contract binding to its selected provider.",
  route: "Maps an HTTP method and path to a top-level handler function.",
  get: "Declares an HTTP GET route. Path segments such as `:id` bind to the first handler parameters in URL order. Remaining primitive handler parameters bind from the query string by name. A parameter with a default is optional; one without a default is required. Invalid, duplicate, or unexpected query values return `400 Bad Request` before the handler runs.\n\n```orni\nfunc list_users(page: uint32 = 1, limit: uint16 = 20) -> response {\n    return users.list(page, limit);\n}\n\nroute get \\\"/users\\\" => list_users;\n// GET /users?page=2&limit=50\n```",
  post: "Declares an HTTP POST route with a validated model request body.",
  put: "Declares an HTTP PUT route with a validated model request body.",
  patch: "Declares an HTTP PATCH route with a partial validated model request body.",
  delete: "Declares an HTTP DELETE route.",
  assert: "Stops execution when a boolean condition is false; use it for Orni tests.",
  test: "Groups assertions into a named test scenario. A scenario passes only when every statement in its body succeeds.",
  app: "Configures the application startup from main with app.name(...) and app.port(...).",
};

const primitiveTypes = {
  string: {
    summary: "UTF-8 text with an optional, memory-aware byte capacity.",
    details: [
      "Bare string infers the smallest safe capacity from literals, arguments, defaults, and expressions.",
      "Use string<N> when an API or storage boundary needs an explicit maximum. N is measured in UTF-8 bytes, not characters.",
      "Orni releases string values when their variable, block, or function scope ends.",
    ],
    example: 'let name: string = "Orni";\nlet email: string<254> = "hello@example.com";',
  },
  int: {
    summary: "A signed 64-bit integer with checked arithmetic.",
    details: ["Use int for general counters and calculations when a narrower type does not express the domain.", "Overflow and invalid narrowing produce an Orni error instead of silently changing the value."],
    example: "let workers: int = 8;\nlet total: int = workers + 2;",
  },
  double: {
    summary: "A 64-bit floating-point number for fractional values.",
    details: ["Mixing an integer with double is allowed only when the integer can be represented exactly.", "Use an integer type for identifiers, counts, and values where exact precision matters."],
    example: "let ratio: double = 0.75;",
  },
  bool: {
    summary: "A logical value that is either true or false.",
    details: ["Use bool for conditions, assertions, and flags.", "If conditions and assert expressions must evaluate to bool."],
    example: "let enabled: bool = true;\nif enabled { print \"ready\"; }",
  },
  char: {
    summary: "Exactly one Unicode scalar value.",
    details: ["Use single quotes for char literals.", "A string containing multiple characters cannot be assigned to char."],
    example: "let initial: char = 'O';",
  },
  uuid: {
    summary: "A validated 128-bit universally unique identifier.",
    details: ["A string value assigned to uuid is parsed and validated before execution.", "HTTP CRUD stores use uuid identifiers by default."],
    example: 'let user_id: uuid = "550e8400-e29b-41d4-a716-446655440000";',
  },
  response: {
    summary: "An explicit HTTP result returned by a route handler.",
    details: ["Create one with response.ok, response.created, response.not_found, and related helpers.", "Use response.header to attach validated HTTP headers."],
    example: "func create(input: User) -> response {\n    return response.created(input);\n}",
  },
  api: {
    summary: "In-memory HTTP test API available through orni test.",
    details: ["api.status and api.body execute the declared route without opening a TCP port.", "Use them only in files under tests/."],
    example: 'assert api.status("GET", "/health") == 200;',
  },
  unit: {
    summary: "The absence of a reusable value.",
    details: ["Use unit for functions that perform an action but do not return data.", "A function without an explicit return produces unit."],
    example: 'func log() -> unit { print "done"; }',
  },
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

  const configuredRoot = vscode.workspace.getConfiguration("orni").get("sourcePath");
  const roots = [
    ...(typeof configuredRoot === "string" && configuredRoot ? [vscode.Uri.file(configuredRoot)] : []),
    ...(vscode.workspace.workspaceFolders || []).map((folder) => folder.uri),
  ];
  const visited = new Set();
  for (const root of roots) {
    const uri = vscode.Uri.joinPath(root, ...target.path);
    if (visited.has(uri.fsPath)) continue;
    visited.add(uri.fsPath);
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

/** Appends a Rust-style documentation layout shared by core Orni symbols. */
function appendDocumentation(markdown, { signature, origin = "orni:core", summary, details = [], example, footer }) {
  markdown.appendCodeblock(signature, "orni");
  markdown.appendMarkdown(`\nDeclared in \`${origin}\`\n\n${summary}`);
  if (details.length) {
    markdown.appendMarkdown("\n\n**Details**");
    for (const detail of details) markdown.appendMarkdown(`\n- ${detail}`);
  }
  if (example) {
    markdown.appendMarkdown("\n\n**Example**\n");
    markdown.appendCodeblock(example, "orni");
  }
  if (footer) markdown.appendMarkdown(`\n${footer}`);
}

/** Describes a fixed-width integer including the exact range it accepts. */
function fixedIntegerDocumentation(typeName) {
  const unsigned = typeName.startsWith("uint");
  const bits = BigInt(typeName.match(/\d+/)?.[0] || "64");
  const minimum = unsigned ? 0n : -(2n ** (bits - 1n));
  const maximum = unsigned ? 2n ** bits - 1n : 2n ** (bits - 1n) - 1n;
  return {
    signature: `final type ${typeName}`,
    summary: `A ${unsigned ? "unsigned" : "signed"} ${bits}-bit integer with checked arithmetic and conversion.`,
    details: [
      `Accepted range: ${minimum} to ${maximum}.`,
      "Use the narrowest type that represents the domain when an API, protocol, or storage model has a fixed range.",
      "Arithmetic overflow and narrowing outside this range produce an Orni error.",
    ],
    example: `let value: ${typeName} = 1;`,
  };
}

/** Builds rich hover content for a symbol. */
async function hoverFor(document, position) {
  const symbol = symbolAt(document, position);
  if (!symbol) return undefined;

  const source = document.getText();
  const usageOffset = document.offsetAt(position);
  const symbolOffset = document.offsetAt(symbol.range.start);
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
    appendDocumentation(markdown, {
      signature: builtin.signature,
      summary: builtin.summary,
      details: builtin.details,
    });
    return new vscode.Hover(markdown, symbol.range);
  }

  const responseHelper = responseHelperAt(source, symbolOffset, symbol.name);
  if (responseHelper) {
    const markdown = new vscode.MarkdownString();
    const [section, explanation] = responseHelperGuidance[symbol.name] || [];
    appendDocumentation(markdown, {
      signature: responseHelper[0],
      origin: "orni:http",
      summary: responseHelper[1],
      details: section ? [`**${section}:** ${explanation}`] : [],
      footer: "\nCtrl+Click to open its Rust implementation.",
    });
    return new vscode.Hover(markdown, symbol.range);
  }

  const storeHelper = storeHelperAt(source, symbolOffset, symbol.name);
  if (storeHelper) {
    const markdown = new vscode.MarkdownString();
    appendDocumentation(markdown, {
      signature: storeHelper[0],
      origin: "orni:http",
      summary: storeHelper[1],
      details: storeHelper[2],
      footer: "\nCtrl+Click to open its Rust implementation.",
    });
    return new vscode.Hover(markdown, symbol.range);
  }

  const apiHelper = apiHelperAt(source, symbolOffset, symbol.name);
  if (apiHelper) {
    const markdown = new vscode.MarkdownString();
    const guidance = apiHelperGuidance[symbol.name] || [];
    appendDocumentation(markdown, {
      signature: apiHelper[0],
      origin: "orni:test",
      summary: apiHelper[1],
      details: guidance.length ? [`**${guidance[0]}:** ${guidance[1]}`, `**${guidance[2]}:** ${guidance[3]}`] : [],
      footer: "\nAvailable in files under \`tests/\` when running \`orni test\`.",
    });
    return new vscode.Hover(markdown, symbol.range);
  }

  const keyword = keywordDocumentation[symbol.name];
  if (keyword) {
    return new vscode.Hover(new vscode.MarkdownString(keyword), symbol.range);
  }

  const primitive = primitiveTypes[symbol.name] ||
    (/^(?:u?int)(?:8|16|32|64)$/.test(symbol.name)
      ? fixedIntegerDocumentation(symbol.name)
      : undefined);
  if (primitive) {
    const markdown = new vscode.MarkdownString();
    appendDocumentation(markdown, {
      signature: primitive.signature || `final type ${symbol.name}`,
      ...primitive,
    });
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
    if (functionDeclaration.parameters.length) {
      markdown.appendMarkdown("\n\n**Parameters**");
      for (const parameter of functionDeclaration.parameters) {
        const optional = parameter.includes("=") ? " Optional; its default is used when omitted." : " Required at every call.";
        markdown.appendMarkdown(`\n- \`${parameter}\` —${optional}`);
      }
    } else {
      markdown.appendMarkdown("\n\n**Parameters**\n- None");
    }
    markdown.appendMarkdown(`\n\n**Returns**\n- \`${functionDeclaration.returnType}\``);
    const hasAutomaticString =
      functionDeclaration.returnType === "string" ||
      functionDeclaration.parameters.some((parameter) => /:\s*string(?:\s*=|$)/.test(parameter));
    if (hasAutomaticString) {
      markdown.appendMarkdown(
        "\n\n**Memory**\n- Bare `string` capacities are inferred from arguments, defaults, and the function body for each call.",
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
  const call = prefix.match(/((?:(?:response|api)\.)?[A-Za-z_][A-Za-z0-9_]*)\s*\(([^()]*)$/s);
  if (!call) return undefined;

  const responseHelper = call[1].startsWith("response.")
    ? responseHelpers[call[1].slice("response.".length)]
    : undefined;
  if (responseHelper) {
    const signature = new vscode.SignatureInformation(responseHelper[0], responseHelper[1]);
    const parameter = responseHelper[0].match(/\(([^)]*)\)/)?.[1];
    signature.parameters = parameter ? [new vscode.ParameterInformation(parameter)] : [];
    const help = new vscode.SignatureHelp();
    help.signatures = [signature];
    help.activeSignature = 0;
    help.activeParameter = 0;
    return help;
  }
  const apiHelper = call[1].startsWith("api.")
    ? apiHelpers[call[1].slice("api.".length)]
    : undefined;
  if (apiHelper) {
    const signature = new vscode.SignatureInformation(apiHelper[0], apiHelper[1]);
    signature.parameters = [
      new vscode.ParameterInformation("method"),
      new vscode.ParameterInformation("path"),
      new vscode.ParameterInformation("body?"),
    ];
    const help = new vscode.SignatureHelp();
    help.signatures = [signature];
    help.activeSignature = 0;
    help.activeParameter = Math.min((call[2].match(/,/g) || []).length, 2);
    return help;
  }

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

/** Returns response helper documentation only when the cursor follows response. */
function responseHelperAt(source, usageOffset, symbolName) {
  const before = source.slice(Math.max(0, usageOffset - 160), usageOffset);
  return /response\s*\.\s*$/.test(before) ? responseHelpers[symbolName] : undefined;
}

/** Returns store helper documentation only when the cursor follows a store name. */
function storeHelperAt(source, usageOffset, symbolName) {
  const before = source.slice(Math.max(0, usageOffset - 160), usageOffset);
  return /[A-Za-z_][A-Za-z0-9_]*\s*\.\s*$/.test(before) ? storeHelpers[symbolName] : undefined;
}

/** Returns API test helper documentation only when the cursor follows api. */
function apiHelperAt(source, usageOffset, symbolName) {
  const before = source.slice(Math.max(0, usageOffset - 160), usageOffset);
  return /api\s*\.\s*$/.test(before) ? apiHelpers[symbolName] : undefined;
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
