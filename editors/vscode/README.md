# Orni Language for Visual Studio Code

This extension provides basic editor support for `.orni` files:

- Orni file icons for light and dark themes;
- syntax highlighting;
- hover documentation for keywords, built-ins, functions, and typed variables;
- inline hints for automatically inferred variable types;
- signature help for typed parameters and default values;
- live structured diagnostics with stable codes, full ranges, and exact module paths;
- multiple compiler errors reported in one analysis pass;
- context-aware Quick Fix actions through `Ctrl+.`;
- an Orni-specific `Ctrl+.` shortcut in editors that use a different default keybinding;
- project-aware binding fixes that update the executable composition file;
- provider skeleton generation from contract methods when no implementation exists;
- unused-import warnings with a one-click removal;
- reusable-module validation through `Ctrl+.` or the Command Palette;
- warnings for unused variables;
- rich documentation for primitive types;
- Ctrl+Click navigation to function and variable declarations;
- Ctrl+Click navigation from imports to their module files;
- Ctrl+Click navigation and completion for namespaced module members;
- auto-import completion and Quick Fixes for unresolved public members;
- module hover summaries with public functions and values;
- deterministic import organization through `Ctrl+.`;
- selective imports with completion, navigation, warnings, and Quick Fixes;
- Ctrl+Click navigation from built-ins and language keywords to their Rust implementation;
- line comment toggling with `//`;
- automatic brackets, parentheses, and quotes;
- indentation and bracket matching.

## Local installation

Package the extension from this directory:

```sh
npx @vscode/vsce package --allow-missing-repository
```

Install the generated VSIX:

```sh
code --install-extension orni-language-0.7.6.vsix
```

Alternatively, use **Extensions: Install from VSIX...** from the Command
Palette in Visual Studio Code or Cursor. Reload the editor after installation.

The preview also supports Antigravity. `Ctrl+.` is explicitly registered for
Orni files through a layout-independent physical Period key and the
**Orni: Quick Fix** command. The compiler is included in the VSIX; Rust and Cargo are only needed
to build the extension, not to use it. A VS Code 1.95-compatible editor is required.

Use `orni.toml` to declare the application entry and source directory. Navigation,
auto-imports, and diagnostics use that source root even from nested modules.
`Orni: Validate Application` saves open files and checks the manifest entry.
Live checks include unsaved Orni buffers without writing them to disk.
The extension also analyzes unopened workspace files automatically at startup.
Subsequent edits recheck changed files and their transitive importers, using
content-based graph fingerprints to reuse unchanged results. New or deleted
files and manifest/configuration changes trigger workspace discovery again.
Use **Orni: Analyze Workspace** to refresh all
Problems manually. Errors remain listed after closing the source editor.

Quick Fixes are available only when a safe automatic edit exists. An intentional
cycle or runtime failure may require a manual code change. The command explains
when no automatic correction is available instead of silently doing nothing.

Every diagnostic also offers **Explain how to fix** in the Quick Fix menu.
It shows diagnostic-specific steps and examples without changing the source.
The same guidance appears in the error hover and the Problems details.
Guidance covers syntax, types, numeric ranges, UTF-8 capacities, modules,
contracts, dependency graphs, warnings, and runtime resource boundaries.
Specific `ORNI-E` codes select guidance independently of message wording.
Legacy phase-wide diagnostics still receive compatible message-based help.

Set `orni.compilerPath` to an installed native executable to override the bundled
compiler. Leave it empty for the default. Analysis is serialized, debounced,
cancelled when superseded, and bounded by a 15-second timeout.

Run `npm ci`, `npm test`, and `npm run test:host` for helper and real editor tests.
Packaging runs `vscode:prepublish`, which builds and copies the release WASM
compiler into the extension automatically.

To run the same integration suite against an installed compatible editor,
set `ORNI_EDITOR_EXECUTABLE` to its executable before `npm run test:host`.
The test runner creates an isolated temporary workspace and user-data directory;
it does not modify the editor's normal settings or user projects.

## Extension architecture

- `extension.js`: activation and editor registrations.
- `editor-symbols.js` / `editor-navigation.js`: hover, signatures, hints, definitions, and completions.
- `module-actions.js` / `code-actions.js`: import discovery and safe source edits.
- `diagnostic-ui.js`: Quick Fix selection and manual correction help.
- `diagnostics.js`: compiler scheduling, cancellation, caching, and Problems publication.
- `dependency-index.js`: source fingerprints and transitive import ownership without retained source copies.
- `diagnostic-catalog.js` / `diagnostic-guidance.js`: stable guidance and legacy compatibility.
