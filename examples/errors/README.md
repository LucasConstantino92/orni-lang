# Error examples

Each `.orni` file intentionally contains one failure so its diagnostic remains
focused. Run a file from the repository root, for example:

```sh
cargo run --target wasm32-wasip1 -- examples/errors/03_string_contract.orni
```

The examples cover undefined variables, incompatible types, explicit string
contracts, checked integer overflow, division by zero, recursion depth, and
runaway string growth. An error exit status is the expected result.

Every file starts with an English comment identifying its intentional failure.
Compile-time errors are listed in Problems during workspace analysis, even when
the files are not open. Runtime-limit examples may pass static analysis and
report their intentional failure only when executed; the analyzer never runs
these programs in the background.
