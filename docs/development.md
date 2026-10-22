# Orni development guide

[← Back to Orni](../README.md) · [Editor guide](editor.md)

## Requirements

- Rust stable with the `wasm32-wasip1` target
- Node.js for editor tests and packaging

## Validation

```powershell
cargo fmt --check
cargo test
cargo build --target wasm32-wasip1

cd editors/vscode
npm test
```

Run an example locally:

```powershell
cargo run -- serve examples/experimental/auth_smoke.orni
```

## Architecture

- `src/frontend`: lexer, parser, and type checker
- `src/syntax`: AST, tokens, and Orni values
- `src/runtime`: interpreter, native core APIs, HTTP server, and limits
- `editors/vscode`: extension, navigation, diagnostics, and grammar
- `examples/experimental`: runnable language and HTTP examples

The language is experimental. Contributions that add a new native API should
include parser/type/runtime coverage, hover documentation, Ctrl+Click source
navigation, an example where appropriate, and tests.
