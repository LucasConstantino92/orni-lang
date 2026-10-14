# Changelog

All notable changes to Orni are documented in this file.

The project follows [Semantic Versioning](https://semver.org/).

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
