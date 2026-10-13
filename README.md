# Orni

Orni is an experimental programming language whose name combines the
platypus with the idea of **Oriented Injection**.
Its goal is to explore a small language in which application dependencies are
explicit, easy to replace, and a natural part of the syntax.

## Current status

The first milestone is a minimal vertical slice: read an `.orni` file, parse
its syntax, and execute it. The language currently supports:

- variables declared with `let`;
- functions with parameters and isolated lexical scopes;
- numbers, strings, and booleans;
- `print` statements;
- the `+`, `-`, `*`, and `/` operators;
- comments starting with `//`;
- errors containing line and column information.

```orni
let name = "Orni";
fn greet(subject) {
    print "Hello, " + subject;
}

greet(name);
print 6 * 7;
```

Run the example with:

```sh
cargo run -- examples/hello.orni
```

Run the test suite with:

```sh
cargo test
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
2. Add return values and nested blocks to functions.
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
