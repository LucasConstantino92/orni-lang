# Experimental framework features

These examples exercise syntax that is still being validated for Orni 0.8.0.
They are intentionally kept outside the stable guided examples until the API
is approved.

Run the contracts and providers example from the repository root:

```sh
cargo run --target wasm32-wasip1 -- run examples/experimental/08_contracts.orni
```

The `framework_project` directory separates the same concepts into application,
contract, and provider modules. It also demonstrates a provider receiving
another contract automatically:

```orni
provider ConsoleAudit(logger: Logger) for Audit {
    func record(message: string) -> unit {
        logger.log("AUDIT: " + message);
    }
}
```

```sh
cargo run --target wasm32-wasip1 -- run examples/experimental/framework_project/main.orni
```

## Dependency graph diagnostics

The `errors` directory contains intentionally invalid dependency graphs. Open
either file in the editor to see the complete diagnostic in the Problems panel.

The missing binding example reports the whole path instead of only the final
contract:

```text
Audit -> Logger -> Clock
```

The cycle example is rejected before execution and reports:

```text
Audit -> Logger -> Audit
```

The unused binding example remains valid, but reports `ORNI-W004` because no
function or provider asks Orni to inject the configured contract.

The `available_binding_fix.orni` example has a provider ready for its missing
binding. Place the cursor on the underlined contract and press `Ctrl+.` to let
the editor add the matching `bind` statement.

If that example has already been fixed, use `errors/pending_binding_fix.orni`
for a fresh intentional missing binding. Press `Ctrl+.` or run **Orni: Quick Fix**
from the Command Palette. Workspace analysis reports these intentional semantic
errors before their files are opened. Header comments identify the expected
failure and distinguish warnings from runtime-only examples.

## Project root and validation

`framework_project/orni.toml` establishes a stable source root. This makes both
application checks and independent nested-module checks work:

```sh
cargo run --target wasm32-wasip1 -- check examples/experimental/framework_project
cargo run --target wasm32-wasip1 -- check-module examples/experimental/framework_project/providers/console_audit.orni
```

Contract methods currently cannot declare default values. The selected provider
graph must have complete bindings, while unused alternative providers do not
need to be configured. `contract_default.orni` and `integer_range.orni` demonstrate
analysis errors that are now caught before execution.
