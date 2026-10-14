# Multi-file module project

This example shows a small application importing one public function from a
nested module. Run it from the repository root:

```sh
cargo run --target wasm32-wasip1 -- run examples/module_project/main.orni
```

Try changing `services.greeting` to `services.greetng`. The compiler will
suggest the closest module file. In the editor, remove the import and use
`greet`; `Ctrl+.` can add the selective import automatically.
