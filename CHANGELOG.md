# Changelog

All notable changes to Orni are documented in this file.

The project follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.9.4] - 2026-10-23

### Added

- `app.timeout(Duration(...))` for a named application-level request deadline,
  returning a safe `408 Request Timeout` when a request outlives its boundary.
- Automatic `X-Request-Id` response headers for HTTP responses, allowing a
  client error to be correlated with one server log entry.
- `app.log_requests(true)` for concise development logs containing the request
  id, method, path, status, and elapsed duration.
- Graceful native-server shutdown handling: new connections stop first and
  active bounded requests are allowed to finish before the process exits.
- `request_timeout_milliseconds` as a project-level HTTP default and an
  `operations.orni` runnable example for observing IDs, deadlines, and logs.

## [0.9.3] - 2026-10-22

### Added

- Automatic ETag revisions for model-backed store resources and optional atomic `If-Match` preconditions for PUT, PATCH, and DELETE operations.
- Stable `412 Precondition Failed` responses with `ORNI_HTTP_PRECONDITION_FAILED` when a client attempts to write an outdated resource representation.
- Conditional GET support through `If-None-Match`, returning `304 Not Modified` without resending an unchanged model-backed resource.
- `orni dev [project-directory]` so a local API can be started without changing the terminal working directory.
- `api.header(...)` and `api.status_with(...)` for in-memory assertions over response and request headers, including ETag and 304 behavior.
- Explicit `app.cors(origin)` configuration with route-aware browser `OPTIONS` preflight responses and a safe allowlist for framework request headers.
- Signed, expiring HMAC-SHA256 Bearer tokens through `app.auth(...)`, `auth.token(...)`, `app.require_auth(...)`, and `auth.user()`.
- Standard `401 Unauthorized` plus `WWW-Authenticate: Bearer` handling for missing, expired, and modified tokens before protected handlers run.
- Separate refresh tokens with a seven-day default lifetime, optional per-app expiry, dedicated refresh routes, and access-token renewal through `auth.refresh()`.
- In-memory refresh-token rotation, replay detection, subject-level session revocation, and `app.logout(...)` routes.
- Native `size` values through `Size(bytes: ...)`, `Size(kilobytes: ...)`, `Size(megabytes: ...)`, and `Size(gigabytes: ...)`, with checked byte normalization and editor documentation/navigation.
- Native `duration` values through `Duration(milliseconds: ...)`, `Duration(seconds: ...)`, `Duration(minutes: ...)`, `Duration(hours: ...)`, and `Duration(days: ...)`, required by time-sensitive application configuration.
- `app.body_limit(Size(...))` and `app.cache_limit(Size(...))` for per-application bounded request parsing and response-cache memory.
- Duration-based named authentication and brute-force protection configuration, secure refresh cookies, and active refresh-session tracking with oldest-session eviction.
- A complete documented local authentication lifecycle covering login, protected requests, refresh rotation, and logout, plus explicit in-memory persistence boundaries.

## [0.9.2] - 2026-10-19

### Added

- Strict JSON media-type enforcement for HTTP routes that receive typed model bodies.
- Clear `415 Unsupported Media Type` and empty-body diagnostics before POST handlers run.
- Defensive HTTP request framing validation for duplicate body headers and unsupported transfer encodings.
- `response.json(...)` and `response.created_json(...)` for stable `{ "data": ... }` JSON success envelopes.
- `response.collection(...)` for `{ "data": [...], "meta": { ... } }` collection responses without changing existing `store.list` output.
- Cursor collection windows through `store.list(limit, cursor)`, with bounded reads, `has_more`, and `next_cursor` instead of mandatory page totals.
- Declared cursor search through `store.search(...)`, including normalized text matching, optional declared sorting, and cursors bound to their filter and ordering state.
- `Accept` negotiation for typed JSON body routes, including `406 Not Acceptable` before a rejected POST mutates storage.
- Declarative `email` and `unique` model field rules, with JSON validation and `409 Conflict` enforcement for create, replace, and patch.
- Automatic `Location` headers for successful model-backed POST creation.
- Optional five-minute `Idempotency-Key` replay protection for POST retries, including conflict detection when a key is reused with a different body.
- Bounded, short-lived GET response caching keyed by the complete URL, with `Cache-Control: no-store` support and automatic invalidation after successful writes.
- Per-project HTTP cache configuration through `cache_ttl_seconds` and `max_cache_entries`.
- `app.protect(...)` for per-IP brute-force protection on explicit sensitive routes, with safe defaults, temporary lockouts, neutral `429` responses, and reset after successful authentication.
- Declarative `min(...)`, `max(...)`, and `optional` model field rules for typed JSON bodies, with field-specific `400` validation details.
- Literal model defaults for POST and PUT bodies, plus `one_of(...)` string choices for closed values such as roles and statuses.

## [0.9.1] - 2026-10-19

### Added

- Typed GET query parameters that bind to handler parameters after URL path parameters.
- Optional query parameters through normal function defaults, with support for `string`, integer types, `double`, `bool`, `char`, and `uuid`.
- Safe query percent-decoding plus clear `400 Bad Request` responses for missing, invalid, duplicate, or unexpected values.
- Paginated `store.list(page, limit)` responses with bounded page sizes and JSON metadata.
- Optional model-field sorting for paginated lists and configurable native HTTP connection limits.
- `api.contains(...)` for concise assertions against JSON response fragments.
- Default configurable per-IP HTTP request rate limiting for every route.
- Detailed editor hover documentation for `get`, including query parameter behavior and an executable example.

## [0.9.0] - 2026-10-15

### Added

- The first local HTTP application workflow with `orni new`, `orni dev`, and `orni serve`.
- Project scaffolding with `orni.toml`, a `src/` entry point, a health route, and executable test files.
- Optional Dart-like application startup through `func main() -> unit` with `app.name(...)` and `app.port(...)` configuration.
- Typed HTTP routes for `GET`, `POST`, `PUT`, `PATCH`, and `DELETE`.
- Typed `model` declarations, JSON request-body validation, and model-backed HTTP responses.
- In-memory `store` CRUD operations: `list`, `create`, `find`, `replace`, `patch`, and `delete`.
- UUID support for route parameters and default CRUD resource identifiers.
- `orni test` with in-memory `api.status(...)` and `api.body(...)` route assertions.
- HTTP-aware editor syntax, hovers, signatures, navigation, start/stop/restart commands, and project actions.
- Structured HTTP diagnostics and runtime errors for invalid routes, request bodies, path parameters, and server startup.

### Changed

- The README and experimental examples now present Orni as a server framework with a focused language and Rust runtime.
- The compiler, parser, type checker, runtime, CLI, and editor now share the application and HTTP model required by `.orni` projects.

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
[0.9.0]: https://github.com/LucasConstantino92/orni-lang/releases/tag/v0.9.0
[0.9.2]: https://github.com/LucasConstantino92/orni-lang/releases/tag/v0.9.2
[0.9.3]: https://github.com/LucasConstantino92/orni-lang/releases/tag/v0.9.3
[0.9.4]: https://github.com/LucasConstantino92/orni-lang/releases/tag/v0.9.4
