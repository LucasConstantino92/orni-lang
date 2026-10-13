# Orni

Orni is an experimental programming language whose name combines the
platypus with the idea of **Oriented Injection**.
Its goal is to explore a small language in which application dependencies are
explicit, easy to replace, and a natural part of the syntax.

## Current status

The first milestone is a minimal vertical slice: read an `.orni` file, parse
its syntax, and execute it. The language currently supports:

- variables declared with `let`, with optional type annotations;
- functions with typed parameters, optional defaults, typed return values, and isolated lexical scopes;
- comparisons with `==`, `!=`, `>`, `>=`, `<`, and `<=`;
- boolean negation with `!`;
- `if`/`else` statements and nested block scopes;
- numbers, strings, and booleans;
- `print` statements;
- the `+`, `-`, `*`, and `/` operators;
- comments starting with `//`;
- errors containing line and column information.

The primitive types use lowercase names and have explicit runtime behavior:

- `int` is a signed 64-bit integer with checked arithmetic;
- `double` is a 64-bit floating-point number;
- `bool` stores `true` or `false`;
- `char` stores one Unicode scalar value;
- `string` stores dynamically sized UTF-8 text;
- `unit` represents the absence of a value.

Fixed-width integers such as `int8`, `int16`, and their unsigned variants are
planned together with checked conversions. Bounded strings and collections
will address allocation limits; integer width alone does not prevent memory
leaks.

```orni
let name: string = "Orni";
func greet(subject: string, greeting: string = "Hello") -> string {
    return greeting + ", " + subject;
}

print greet(name);
print 6 * 7;
```

## Playground

Open any `.orni` file and press `Ctrl+Shift+B` in a compatible editor such as
Antigravity, Visual Studio Code, or Cursor. The default build task runs the
active file through the Orni interpreter. The separate `Orni: Run playground`
task always runs `playground/main.orni`.

You can also run it from a terminal:

```sh
cargo run --target wasm32-wasip1 -- playground/main.orni
```

Validate a file without executing it with:

```sh
cargo run --target wasm32-wasip1 -- --check playground/main.orni
```

The development runner uses WASI so contributors can execute Orni without a
platform-specific native linker. It requires the Rust `wasm32-wasip1` target
and Node.js. Install the target once with:

```sh
rustup target add wasm32-wasip1
```

The `examples` directory preserves the language's learning path:

1. `01_basics.orni` introduces values, variables, and arithmetic.
2. `02_functions.orni` introduces functions and return values.
3. `03_conditionals.orni` introduces comparisons, branches, and block scopes.

## Visual Studio Code support

The extension source under `editors/vscode` registers `.orni` files, provides
light and dark Orni file icons, syntax highlighting, comment toggling, bracket
matching, automatic closing pairs, and indentation rules.

Run the example with:

```sh
cargo run -- examples/hello.orni
```

Run the test suite with:

```sh
cargo test --target wasm32-wasip1
```

## Documentation

Public APIs use Rust documentation comments (`///`). The crate enables
`missing_docs` warnings so newly exposed APIs are not left undocumented.
Generate documentation for both public and internal items with:

```sh
cargo doc --no-deps --document-private-items --open
```

Editors powered by rust-analyzer display these comments on hover and navigate
to the documented declaration with Ctrl+Click.

## Roadmap

1. Define the minimum syntax for modules, contracts, providers, and injection.
2. Add conditional execution and nested blocks.
3. Build Orni's first dependency injection container.
4. Expose PostgreSQL through an explicit adapter while keeping SQL visible.
5. Extend the CLI with `orni run`, `orni check`, and richer diagnostics.

PostgreSQL will not be part of the interpreter core. It will be an adapter in
the standard library. This separation keeps the language small, allows the
database to be replaced in tests, and does not hide SQL from the program.

## Contributing

Orni is at an early experimental stage. The public language design has not
stabilized yet, so discussions and small, focused changes are preferred.
Code, documentation, commit messages, and public project communication should
be written in English.
