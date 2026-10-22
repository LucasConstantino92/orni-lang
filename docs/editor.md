# Orni editor guide

[← Back to Orni](../README.md) · [Language guide](language.md)

The Orni VS Code-compatible extension provides `.orni` highlighting, live
diagnostics, hover documentation, signatures, Ctrl+Click navigation, and quick
fix guidance.

Set the executable path when using a local install:

```json
"orni.compilerPath": "C:\\Users\\you\\.cargo\\bin\\orni.exe"
```

To navigate from native APIs such as `Duration`, `Size`, `response.ok`, or
`store.create` to the documented Rust implementation, also set:

```json
"orni.sourcePath": "C:\\Projects\\Rust\\orni-lang"
```

Hover over a type or helper to see its accepted parameters, behavior, examples,
and resource constraints. Ctrl+Click opens either the source declaration in
your application or the matching native implementation in the Orni runtime.

Install a packaged extension with **Extensions: Install from VSIX...**, then
reload the editor window after an update.
