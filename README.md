<p align="center">
  <img src="editors/vscode/icons/orni-file.svg" width="140" alt="Orni logo">
</p>

<h1 align="center">Orni</h1>

<p align="center">
  <strong>A server framework powered by a small, safety-oriented language built in Rust.</strong>
</p>

<p align="center">
  Predictable types, bounded resources, helpful diagnostics, and dependency
  injection as a language-level concept.
</p>

<p align="center">
  <img alt="Status: experimental" src="https://img.shields.io/badge/status-experimental-f59e0b">
  <img alt="Rust edition 2024" src="https://img.shields.io/badge/Rust-2024-000000?logo=rust">
  <img alt="Editor support" src="https://img.shields.io/badge/editor-live%20diagnostics-6366f1">
</p>

> [!IMPORTANT]
> Orni is an early-stage language experiment. Its syntax and runtime APIs may
> change while the core design is being established.

## Why Orni?

Orni is being built as a server application framework with its own focused
language. Safe defaults should be concise instead of verbose. The name combines
the platypus with the idea of **Oriented Injection**: application dependencies
should eventually be explicit, replaceable, and natural to express in source
code.

The project is divided conceptually into three layers:

- **Orni Framework** — the future application model for HTTP, configuration,
  validation, observability, persistence adapters, and dependency injection;
- **Orni Language** — the compact `.orni` syntax used to describe application
  behavior and dependencies;
- **Orni Runtime** — the Rust implementation responsible for analysis, safe
  execution, resource boundaries, and framework services.

Today, the language and runtime form the working foundation. Framework services
will be added incrementally on top of them.

The current foundation focuses on:

- automatic string-capacity and integer-width inference;
- optional explicit bounds for public contracts and untrusted input;
- checked arithmetic and conversions;
- lexical scopes backed by Rust ownership;
- configurable runtime resource limits;
- structured diagnostics with stable codes, severity, source paths, and ranges;
- first-class editor support for `.orni` files.

Version 0.9.0 adds the first local HTTP application workflow: project creation,
startup configuration, typed routes, in-memory CRUD stores, UUID identifiers,
and executable Orni tests. Explore the guided examples in
[`examples/experimental`](examples/experimental).

## A first Orni program

```orni
/// Creates a greeting with capacities inferred for each call.
func greet(name: string, greeting: string = "Hello") -> string {
    return greeting + ", " + name;
}

let project = "Orni"; // inferred as string<4>
let answer = 6 * 7;   // inferred integer storage

print greet(project);
print answer;
```

Output:

```text
Hello, Orni
42
```

## Language features

- Variables declared with `let` and optional type annotations
- Functions with typed parameters, defaults, return values, and isolated scopes
- Call-site inference for automatic string parameters and returns
- `if`/`else` statements and nested blocks
- Comparisons, boolean negation, and arithmetic operators
- UTF-8 strings, Unicode characters, booleans, integers, and doubles
- Documentation comments written with `///`
- Checked overflow, division-by-zero detection, and bounded execution

### Primitive types

| Type | Behavior |
| --- | --- |
| `string` | UTF-8 string whose capacity is inferred |
| `string<N>` | UTF-8 string with an explicit maximum of `N` bytes |
| `int` / `int64` | Signed 64-bit integer with checked arithmetic |
| `int8`, `int16`, `int32` | Smaller signed integer ranges |
| `uint8`, `uint16`, `uint32`, `uint64` | Explicit unsigned integer ranges |
| `double` | 64-bit floating-point number |
| `bool` | `true` or `false` |
| `char` | One Unicode scalar value |
| `uuid` | A validated 128-bit universally unique identifier |
| `response` | An explicit HTTP result created by a route handler |
| `unit` | Absence of a reusable value |

### Typed models

`model` declares a named data shape. It is the foundation for typed JSON bodies,
responses, and storage in the HTTP preview:

```orni
model User {
    id: uuid;
    name: string<80>;
    email: string<254>;
}

model CreateUser {
    name: string<80>;
    email: string<254>;
}

model UpdateUser {
    name: string<80>;
    email: string<254>;
}

model PatchUser {
    name: string<80>;
    email: string<254>;
}
```

Models currently accept built-in value types only. A `POST` route can receive
one model as its final handler parameter; Orni validates the JSON object before
the handler runs. Nested models and persistence will be introduced with the
CRUD API.

## Smart inference, explicit control

Orni chooses compact types when it has enough information:

```orni
let name = "Orni";            // string<4>
let status: string = "Ready"; // string<5>
let retries = 3;              // int8
let port = 8080;              // int16
let users = 250000;           // int32
let distance = 9000000000;    // int
let maximum = 18446744073709551615; // uint64
```

Negative literals include their sign during inference: `-128` is `int8`.
Positive literals above `int64` use `uint64`; values outside both 64-bit ranges
are rejected. Negating unsigned variables requires an explicit signed
declaration that can contain the value. Known out-of-range literal conversions
are rejected during analysis, and computed conversions remain checked at runtime.
Integer widths currently enforce ranges in the interpreter; they do not yet
represent packed native integer storage.

Bare `string` parameters and returns are specialized at each call site. The
compiler propagates argument and default capacities through the function body:

```orni
func greet(name: string, greeting: string = "Hello") -> string {
    return greeting + ", " + name;
}

let first: string<11> = greet("Orni");
let second: string<13> = greet("Orni", "Welcome");
```

Inference is optional. Use `string<N>` whenever an API must enforce a maximum:

```orni
func normalize_username(value: string<32>) -> string<32> {
    return value;
}
```

## Resource safety

Orni uses the host runtime's Rust ownership model. Local bindings are released
when their block or function ends, and immutable strings are reference-counted
so variable lookups and function arguments do not repeatedly copy their data.
String concatenation calculates its final UTF-8 size first and performs one
allocation.

Default runtime boundaries protect applications from runaway resource use:

| Resource | Default limit |
| --- | ---: |
| One generated string | 1 MiB |
| Strings retained by active variables | 8 MiB |
| Captured program output | 1 MiB |
| Simultaneously active variables | 10,000 |
| Function call depth | 128 |
| Evaluated operations | 100,000 |

Applications embedding Orni can apply stricter boundaries through the public
`run_with_limits` Rust API. These protections limit resource exhaustion; real
production workloads should still be measured and profiled.

Compilation is bounded too: 1 MiB per source file, 8 MiB per module graph,
20,000 tokens per file, 100,000 tokens per graph, 256 modules, 64 import levels,
64 syntax/tree levels, and 100,000 semantic-analysis operations. String-return
inference is limited to 64 nested calls. These are compilation safeguards, not
a claim that the entire host process uses a fixed amount of memory.

Function bodies are shared during calls and type inference rather than copied
on every invocation. Active binding and string-byte counters are updated when
scopes enter or leave instead of rescanning every retained value on each `let`.

## Project manifests and dependency injection (preview)

## Start a project

Create an API workspace with a runnable user CRUD starter:

```sh
orni new my_api
cd my_api
orni dev
```

`orni new my_api --empty` creates only a `main.orni`, manifest, `/health`
route, and one sample test in `tests/`. Both templates use an optional Dart-like
entry point:

```orni
func main() -> unit {
    app.name("Orni API");
    app.port(3000);
    print "Starting Orni API";
}
```

When present, `main` is the application entry point. It must use the signature
`func main() -> unit`; `orni run` executes it after top-level statements, while
`orni dev` and `orni serve` execute it once before starting the local server.
HTTP routes remain top-level declarations.

`app.name(...)` controls the startup label and `app.port(...)` changes the local
server port. Both are optional; the default address is `127.0.0.1:3000`.

Run application checks with `assert` in any `.orni` file under `tests/`:

```orni
import services.health.{health};

assert health() == "OK";
```

Then execute every test file with:

```sh
orni test
```

Tests import reusable modules from `src/`, not `src/main.orni`. The entry file
can declare routes, stores, and startup configuration, so it intentionally
cannot be imported as a reusable module. Move testable behavior into a module
such as `src/services/health.orni`, import it from both `main.orni` and the
test file, and keep `main.orni` as the application composition root.

Place `orni.toml` at the application root:

```toml
[project]
name = "my_application"
entry = "src/main.orni"
source = "src"

[runtime]
max_operations = 100000
max_string_bytes = 1048576
```

Imports resolve relative to their file or to `project.source`. If both paths
exist and differ, Orni reports an ambiguity. The nearest manifest also lets a
nested provider module be checked independently. Project paths must stay inside
the project directory. Unknown settings and invalid limits are errors.

`orni run`, `orni serve`, `orni check`, and `orni test` use the manifest entry when no path is provided.
The CLI applies runtime settings from the manifest; an embedding application's
explicit `run_file_with_limits` settings take precedence.

The experimental HTTP preview supports top-level GET, POST, PUT, PATCH, and DELETE routes. GET
handlers may return a string or an explicit `response`; POST handlers receive
one model body as their final parameter and return `response`. Route parameters
use `:name` and are passed to matching handler parameters in URL order.
The handler type controls safe conversion; use `int`,
fixed-width integers such as `uint16`, `double`, `bool`, `char`, or `uuid` when a route
needs a typed value. Invalid input returns `400 Bad Request` without calling the
handler. `orni serve` starts a native Rust server on
`127.0.0.1:3000`; it is not available through the WASI editor/compiler target.
See [`examples/experimental/http_server`](examples/experimental/http_server).

```orni
func hello(name: string) -> string {
    return "Hello, " + name + "!";
}

route get "/hello/:name" => hello;
```

`GET /hello/Orni` responds with `Hello, Orni!`. Literal routes take precedence
over parameterized routes. Query strings are ignored in this preview.

For a complete in-memory CRUD endpoint, declare a model with `id: uuid` and a
`store`. Missing or extra fields, invalid field types, oversized strings, and
invalid UUIDs return `400 Bad Request` before the function runs:

```orni
model User {
    id: uuid;
    name: string<80>;
    email: string<254>;
}

store users: User;

func create_user(input: CreateUser) -> response {
    return users.create(input);
}

route post "/users" => create_user;

func list_users() -> response { return users.list(); }
route get "/users" => list_users;

func get_user(id: uuid) -> response { return users.find(id); }
func update_user(id: uuid, input: UpdateUser) -> response { return users.replace(id, input); }
func patch_user(id: uuid, input: PatchUser) -> response { return users.patch(id, input); }
func delete_user(id: uuid) -> response { return users.delete(id); }

route get "/users/:id" => get_user;
route put "/users/:id" => update_user;
route patch "/users/:id" => patch_user;
route delete "/users/:id" => delete_user;
```

`create` generates the UUID in the server, so `POST /users` receives only the
client-owned `CreateUser` fields and returns the complete `User`. The collection lives only while `orni serve` is running: `create` returns
`201`, `find` returns `200` or `404`, `replace` returns `200` or `404`, and
`delete` returns `204` or `404`. A duplicate UUID returns `409 Conflict`.
For `PATCH`, every declared field is optional in the JSON body; only supplied
fields are changed, and an empty object is rejected.

Store operations also include `list`, which returns every retained model as a
JSON array. Route handlers can produce explicit business responses with
`response.ok(body)`, `response.created(body)`, `response.bad_request(body)`,
`response.not_found(body)`, or `response.no_content()`.

```orni
func user(id: uint16) -> string {
    return "User #" + id;
}

route get "/users/:id" => user;
```

`GET /users/42` calls `user` with an actual `uint16`; `/users/70000` returns
`400 Bad Request` because the value cannot fit in that type.

```orni
contract Clock {
    func now() -> int;
}

provider SystemClock for Clock {
    func now() -> int {
        return 0;
    }
}

bind Clock to SystemClock;

func start(clock: Clock) -> unit {
    print clock.now();
}

start();
```

Contracts define method signatures; providers implement them; bindings select
the implementations injected into functions. Provider dependencies are checked
through the selected binding graph. Unselected providers can remain available
without bindings for their dependencies. Contract methods currently require
explicit arguments and cannot declare defaults.

## Modules

Applications can split functions and read-only module values across `.orni`
files. Imports use a dotted path relative to the file that declares them:

```orni
// services/greeting.orni
let framework_name = "Orni";
let _separator = ", ";

func _normalize(name: string) -> string {
    return name;
}

func greet(name: string) -> string {
    return "Hello" + _separator + _normalize(name);
}
```

```orni
// main.orni
import services.greeting as welcome;

print welcome.greet("Orni");
print welcome.framework_name;
```

Public members can also be selected explicitly and used without a namespace:

```orni
import services.greeting.{greet, framework_name};

print greet("Orni");
print framework_name;
```

Selective imports cannot include underscore-prefixed private members or be
combined with `as`. Every selected name must exist in the direct module API and
must not conflict with another declaration or imported name in the same file.

The resolver loads imports recursively, deduplicates repeated modules, and
rejects circular dependencies, duplicate declarations, and namespace
collisions. Declarations are public by default. A name that starts with `_` is
private to its module and cannot be accessed through an import, including
through nested namespaces. Regular imports use a namespace, and `as` can choose
its local name; selective imports place only their listed public members in the
local scope. Imported files may contain imports, contracts, providers, functions, and immutable
module variables; executable statements remain exclusive to the entry file.
Diagnostics preserve the exact source module so the editor can open the
failing file directly from the Problems panel.

Check a reusable module independently from an executable entry file:

```sh
cargo run -- check-module examples/modules/greeting.orni
```

This stricter check rejects executable top-level statements. In the editor,
the same action is available as **Orni: Check Current File as Module** through
`Ctrl+.` and the Command Palette. Unused module imports are reported as
warnings and can be removed with a Quick Fix.

## Diagnostics

Orni diagnostics are structured values exposed through the Rust API. Each one
contains a stable code, severity, phase, optional source path, complete source
range, and human-readable message. Independent semantic errors are collected
in one analysis pass so fixing one issue does not immediately reveal another.

```text
Semantic error [ORNI-E103] at line 1, column 7 to line 1, column 20: undefined variable 'first_missing'
Semantic error [ORNI-E103] at line 2, column 7 to line 2, column 21: undefined variable 'second_missing'
```

Current diagnostic families:

| Code | Meaning |
|---|---|
| `ORNI-E101`–`ORNI-E151` | Specific error identities, shared across compiler phases |
| `ORNI-L001` | Lexical error |
| `ORNI-S001` | Syntax error |
| `ORNI-T001` | Type or name analysis error |
| `ORNI-M001` | Module graph or visibility error |
| `ORNI-R001` | Runtime safety error |
| `ORNI-W001` | Unused module import warning |
| `ORNI-W002` | Unused selectively imported member warning |
| `ORNI-W003` | Unused private function warning |
| `ORNI-W004` | Contract binding that is never injected |

Embedding applications can call `diagnose_file` or `diagnose_module` to read
non-fatal diagnostics. Failed checks expose every collected error through
`OrniError::diagnostics()`.

Specific error codes are selected where the compiler creates the diagnostic,
not inferred from its English message. The editor uses these codes for **How to
fix** guidance. Phase-wide codes remain fallbacks; older configured compilers
retain message-based help. See the [diagnostic code catalog](docs/diagnostic-codes.md).

## Quick start

### Requirements

- [Rust](https://www.rust-lang.org/tools/install)
- Node.js, used by the WASI development runner
- The Rust `wasm32-wasip1` target
- A native C/C++ linker to run the experimental HTTP server (`link.exe` from the MSVC C++ build tools on Windows)

Install the target once:

```sh
rustup target add wasm32-wasip1
```

Run the playground:

```sh
cargo run --target wasm32-wasip1 -- run playground/main.orni
```

Run one of the guided examples:

```sh
cargo run --target wasm32-wasip1 -- run examples/04_type_safety.orni
```

Validate a program without executing it:

```sh
cargo run --target wasm32-wasip1 -- check playground/main.orni
```

## Releases

Every pushed semantic version tag triggers the release workflow. It runs the
Rust and editor test suites, applies Clippy with warnings denied, packages the
editor extension, creates a GitHub Release, and attaches the generated `.vsix`.

Existing tags can be published through **Actions → Release → Run workflow** by
entering the exact tag, such as `v0.7.5`. Release tags are never moved or
rewritten by the workflow.

## Examples

| Example | What it demonstrates |
| --- | --- |
| [`01_basics.orni`](examples/01_basics.orni) | Values, variables, printing, and arithmetic |
| [`02_functions.orni`](examples/02_functions.orni) | Functions, parameters, and return values |
| [`03_conditionals.orni`](examples/03_conditionals.orni) | Comparisons, branches, and block scopes |
| [`04_type_safety.orni`](examples/04_type_safety.orni) | Inference, bounded types, defaults, and documentation |
| [`05_modules.orni`](examples/05_modules.orni) | Namespaces, aliases, public values, and private helpers |
| [`06_selective_imports.orni`](examples/06_selective_imports.orni) | Public members imported directly into the local scope |
| [`module_project`](examples/module_project) | Auto-imports, nested modules, documentation, and typo suggestions |

### Learn from failures

The [`examples/errors`](examples/errors) directory contains intentionally
invalid programs. Each file demonstrates one focused diagnostic:

- undefined variables;
- incompatible types;
- exceeded string contracts;
- integer overflow;
- division by zero;
- excessive recursion;
- runaway string growth.

Run one and inspect the compiler or runtime message:

```sh
cargo run --target wasm32-wasip1 -- examples/errors/03_string_contract.orni
```

An error exit status is expected for these examples.

## Editor experience

The extension in [`editors/vscode`](editors/vscode) supports Antigravity,
Visual Studio Code, and compatible editors. It currently provides:

- Orni icons for `.orni` files;
- syntax highlighting and language-aware indentation;
- live diagnostics before execution;
- context-aware Quick Fix actions through `Ctrl+.`;
- public-member auto-imports and deterministic import organization;
- unused-variable warnings;
- hover documentation and inferred type information;
- inline type hints for inferred variables;
- function signature help;
- Ctrl+Click navigation to declarations;
- Ctrl+Click navigation from imports to their `.orni` module;
- Ctrl+Click navigation and completion for namespaced module members;
- Ctrl+Click navigation from language built-ins to their Rust implementation.

Package the local extension:

```sh
cd editors/vscode
npx @vscode/vsce package --allow-missing-repository
```

Install the generated `.vsix` using your editor's **Install from VSIX** command,
then reload the editor window.

The preview VSIX includes the WASM compiler and its runner, so editor analysis
does not require Rust, Cargo, or the compiler checkout. It requires a compatible
editor based on VS Code 1.95 or newer. Set `orni.compilerPath` to use an installed
native Orni executable instead. Unsaved buffers are checked together, including
modules, without executing the program or saving the files. Analysis is queued,
debounced, and limited to 15 seconds. **Orni: Validate Application** saves and
checks the complete application's manifest entry.

## Development

Run the complete Rust test suite:

```sh
cargo test --target wasm32-wasip1
```

Run the editor support tests:

```sh
npm ci --prefix editors/vscode
npm test --prefix editors/vscode
node --test tools/cli.test.mjs
node tools/package-editor.mjs
npm run test:host --prefix editors/vscode
```

Require warning-free Rust code:

```sh
cargo clippy --all-targets --target wasm32-wasip1 -- -D warnings
```

Generate internal and public Rust documentation:

```sh
cargo doc --no-deps --document-private-items --open
```

All public Rust APIs use `///` documentation comments, and the crate enables
`missing_docs` warnings to keep newly exposed APIs documented.

## Architecture

```text
Orni source (.orni)
        │
        ▼
      Lexer ──► Parser ──► Type checker ──► Interpreter
        │          │              │               │
      Tokens      AST       Diagnostics      Program output
```

| Path | Responsibility |
| --- | --- |
| `src/frontend` | Lexing, parsing, module loading, and semantic type checking |
| `src/syntax` | Tokens, values, and abstract syntax tree nodes |
| `src/runtime` | Environments, resource limits, and interpretation |
| `src/diagnostics.rs` | Errors shared by every compilation stage |
| `src/lib.rs` | Public checking and execution pipeline |
| `editors/vscode` | `.orni` editor integration |

## Roadmap

- [x] Lexer, parser, semantic checker, and interpreter
- [x] Functions, defaults, returns, conditionals, and lexical scopes
- [x] Fixed-width integers and bounded UTF-8 strings
- [x] Smart literal and function-call string inference
- [x] Live editor diagnostics, hover information, and navigation
- [x] Runtime memory, recursion, output, and execution-fuel boundaries
- [x] Namespaced modules, aliases, underscore privacy, and cycle detection
- [x] Developer CLI with `orni run`, `orni check`, and module validation
- [x] Experimental native HTTP CRUD preview with typed JSON and in-memory stores
- [x] Contracts, providers, and dependency injection syntax (unreleased preview)
- [x] Project manifests and standalone editor compiler (unreleased preview)
- [ ] Standard-library adapter model
- [ ] PostgreSQL adapter with visible, explicit SQL
- [ ] Installable production CLI with package manifests and richer diagnostics
- [ ] Benchmarks and memory telemetry

CI validates pushes and pull requests on Windows and Linux before release.
It checks formatting, Rust lints, CLI exit statuses, helper tests, and the real
Extension Host. Release validation additionally requires Cargo, Cargo.lock,
the extension package, and the tag to report the same version.

PostgreSQL will remain outside the interpreter core. It is planned as an
adapter so applications can replace it during tests without hiding SQL from
the program.

## Contributing

Orni is still defining its public language design. Discussions and small,
focused changes are welcome. Code, documentation, commit messages, and public
project communication should be written in English.

## License

Orni is open source software available under the [MIT License](LICENSE).
