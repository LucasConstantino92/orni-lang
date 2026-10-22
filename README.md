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

Orni is an experiment in building backend applications with Rust's safety and
resource awareness behind a compact, readable language. The goal is not to hide
important decisions: it is to make safe boundaries, useful errors, and common
SaaS building blocks easier to start with.

The project has three layers:

- **Orni Framework** — HTTP, configuration, validation, persistence adapters,
  observability, and dependency injection as the framework grows.
- **Orni Language** — the focused `.orni` syntax used to describe application
  behavior.
- **Orni Runtime** — the Rust implementation responsible for analysis, safe
  execution, resource boundaries, and native services.

## A first Orni program

```orni
/// Creates a greeting with capacities inferred for each call.
func greet(name: string, greeting: string = "Hello") -> string {
    return greeting + ", " + name;
}

let project = "Orni";
let answer = 6 * 7;

print greet(project);
print answer;
```

```text
Hello, Orni
42
```

## Start here

```powershell
# Run this from the cloned Orni repository.
cargo install --path .
orni new my_api
cd my_api
orni dev
```

The generated project contains `src/main.orni`, which is the application entry
point. Run `orni --help` to see all local commands.

## Guides

| Guide | Covers |
| --- | --- |
| [Language guide](docs/language.md) | Types, inference, functions, models, modules, and dependency injection |
| [HTTP guide](docs/http.md) | Application startup, CRUD routes, collections, cache, ETags, CORS, and server limits |
| [Authentication guide](docs/authentication.md) | Access tokens, refresh rotation, secure cookies, logout, sessions, and current boundaries |
| [Editor guide](docs/editor.md) | Extension installation, hover, diagnostics, and Ctrl+Click native source navigation |
| [Development guide](docs/development.md) | Repository layout, validation, examples, and contribution expectations |

## Examples

The [`examples/experimental`](examples/experimental) directory contains small,
runnable programs for language features, HTTP, CORS, resource limits, and
authentication. The `auth_smoke.orni` example is the quickest way to explore a
complete local login → profile → refresh → logout flow.

## Current status

Orni currently provides a typed local HTTP workflow, in-memory CRUD stores,
bounded request and cache memory, JSON validation, route protection, CORS,
ETags, token authentication, an Orni test runner, and editor diagnostics.

Stores, caches, rate limits, and refresh sessions are currently in process
memory. They are designed for local development and early prototypes; durable
database and shared-session integrations are planned as the framework evolves.

## Contributing

Contributions are welcome. Please keep source comments and documentation in
English, add tests for behavior changes, and update editor hover/navigation for
every new native API.

See the [development guide](docs/development.md) before opening a change.

## License

This project is distributed under the license in [LICENSE](LICENSE).
