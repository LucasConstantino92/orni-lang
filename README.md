<p align="center">
  <img src="editors/vscode/icons/orni-file.svg" width="140" alt="Orni logo">
</p>

<h1 align="center">Orni</h1>

<p align="center">
  <strong>A server framework powered by a small, safety-oriented language built in Rust.</strong>
</p>

<p align="center">
  Predictable types, bounded resources, helpful diagnostics, and dependency
  injection as a future language-level concept.
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
- useful errors with source line and column information;
- first-class editor support for `.orni` files.

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
| `unit` | Absence of a reusable value |

## Smart inference, explicit control

Orni chooses compact types when it has enough information:

```orni
let name = "Orni";            // string<4>
let status: string = "Ready"; // string<5>
let retries = 3;              // int8
let port = 8080;              // int16
let users = 250000;           // int32
let distance = 9000000000;    // int
```

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

## Quick start

### Requirements

- [Rust](https://www.rust-lang.org/tools/install)
- Node.js, used by the WASI development runner
- The Rust `wasm32-wasip1` target

Install the target once:

```sh
rustup target add wasm32-wasip1
```

Run the playground:

```sh
cargo run --target wasm32-wasip1 -- playground/main.orni
```

Run one of the guided examples:

```sh
cargo run --target wasm32-wasip1 -- examples/04_type_safety.orni
```

Validate a program without executing it:

```sh
cargo run --target wasm32-wasip1 -- --check playground/main.orni
```

## Examples

| Example | What it demonstrates |
| --- | --- |
| [`01_basics.orni`](examples/01_basics.orni) | Values, variables, printing, and arithmetic |
| [`02_functions.orni`](examples/02_functions.orni) | Functions, parameters, and return values |
| [`03_conditionals.orni`](examples/03_conditionals.orni) | Comparisons, branches, and block scopes |
| [`04_type_safety.orni`](examples/04_type_safety.orni) | Inference, bounded types, defaults, and documentation |

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
- unused-variable warnings;
- hover documentation and inferred type information;
- inline type hints for inferred variables;
- function signature help;
- Ctrl+Click navigation to declarations;
- Ctrl+Click navigation from language built-ins to their Rust implementation.

Package the local extension:

```sh
cd editors/vscode
npx @vscode/vsce package --allow-missing-repository
```

Install the generated `.vsix` using your editor's **Install from VSIX** command,
then reload the editor window.

## Development

Run the complete Rust test suite:

```sh
cargo test --target wasm32-wasip1
```

Run the editor support tests:

```sh
node --test editors/vscode/test/language-support.test.js
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
| `src/frontend` | Lexing, parsing, and semantic type checking |
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
- [ ] Modules and imports
- [ ] Contracts, providers, and dependency injection syntax
- [ ] Standard-library adapter model
- [ ] PostgreSQL adapter with visible, explicit SQL
- [ ] Production CLI with `orni run`, `orni check`, and richer diagnostics
- [ ] Benchmarks and memory telemetry

PostgreSQL will remain outside the interpreter core. It is planned as an
adapter so applications can replace it during tests without hiding SQL from
the program.

## Contributing

Orni is still defining its public language design. Discussions and small,
focused changes are welcome. Code, documentation, commit messages, and public
project communication should be written in English.
