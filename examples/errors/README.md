# Error examples

Each `.orni` file intentionally contains one failure so its diagnostic remains
focused. Run a file from the repository root, for example:

```sh
cargo run --target wasm32-wasip1 -- examples/errors/03_string_contract.orni
```

The examples cover undefined variables, incompatible types, explicit string
contracts, checked integer overflow, division by zero, recursion depth, and
runaway string growth. An error exit status is the expected result.
