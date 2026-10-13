# Orni Language for Visual Studio Code

This extension provides basic editor support for `.orni` files:

- Orni file icons for light and dark themes;
- syntax highlighting;
- hover documentation for keywords, built-ins, functions, and typed variables;
- signature help for typed parameters and default values;
- compiler diagnostics after saving an Orni file;
- Ctrl+Click navigation to function and variable declarations;
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
code --install-extension orni-language-0.4.0.vsix
```

Alternatively, use **Extensions: Install from VSIX...** from the Command
Palette in Visual Studio Code or Cursor. Reload the editor after installation.
