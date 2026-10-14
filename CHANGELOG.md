# Changelog

All notable changes to Orni are documented in this file.

The project follows [Semantic Versioning](https://semver.org/).

## [0.8.0] - 2026-10-14

### Added

- Experimental contracts, providers, bindings, and dependency graph diagnostics.
- `orni.toml` project roots, entry points, and runtime limits.
- A compiler bundled with the editor extension and optional `orni.compilerPath`.
- Multi-file unsaved-buffer analysis and an **Orni: Validate Application** command.
- Provider creation and binding Quick Fixes with module-aware imports.
- Diagnostic-specific manual correction steps in Quick Fix, error hover, and Problems details.
- Stable error identities (`ORNI-E101` through `ORNI-E151`) selected at compiler error sites, with message-independent editor guidance and legacy compiler compatibility.
- Content-addressed import graph caching and incremental workspace analysis.
- Compilation resource limits and real Extension Host integration tests.
- Windows/Linux pull-request CI and release version consistency checks.

### Fixed

- WASI process exit codes and Windows path separators.
- Independent nested-module validation and editor module navigation.
- Contract defaults that previously passed analysis but failed at runtime.
- Unselected providers incorrectly requiring dependency bindings.
- Signed literal boundaries, uint64 inference, unsigned negation, and literal narrowing.
- Duplicate parameters and same-scope variable declarations.
- Quoted delimiters and commented declarations in editor symbol discovery.
- Stale diagnostics and ambiguous application correction targets.
- Reusable module checks saving unsaved editor buffers; both application and module checks now use read-only buffer overlays.
- Background document eviction causing repeated analysis, discarded-buffer diagnostics, and transient project-configuration markers.

### Changed

- Function bodies use shared ownership during invocation and inference.
- Editor activation, symbol navigation, code actions, diagnostic presentation, analysis, and dependency indexing have separate modules.
- Runtime binding and retained-string accounting use incremental counters.
- Unknown string escapes are errors in both the compiler and editor inference.
- The standalone editor preview requires VS Code 1.95 or a compatible editor.

## [0.7.6] - 2026-10-14

### Added

- Friendly `orni run`, `orni check`, `orni check-module`, `--help`, and `--version` commands.
- Compiler suggestions for misspelled module files.
- Warnings for unused private functions.
- Editor auto-import completions and Quick Fixes for unresolved public members.
- An **Organize Orni imports** source action.
- Module hovers that list paths, documentation, and public members.
- Project-root validation for canonicalized module paths.
- A guided multi-file module project.

### Changed

- Release automation now uses Node.js 24-compatible GitHub Actions.
- The compiler and editor extension now declare the MIT license explicitly.

[0.7.6]: https://github.com/LucasConstantino92/orni-lang/releases/tag/v0.7.6
[0.8.0]: https://github.com/LucasConstantino92/orni-lang/releases/tag/v0.8.0
