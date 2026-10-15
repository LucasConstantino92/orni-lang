# Experimental framework features

These examples exercise framework syntax that is still being validated. They
are intentionally kept outside the stable guided examples while the API is
being explored.

## Typed models preview

Use `model` to declare the exact data shape that a JSON body, response, or
future store will accept. Fields are currently limited to built-in value types so
their validation and memory boundaries stay explicit:

```orni
model User {
    id: uuid;
    name: string<80>;
    email: string<254>;
}

model CreateUser {
    name: string<80>;
    email: string<254>;
}

model UpdateUser {
    name: string<80>;
    email: string<254>;
}

model PatchUser {
    name: string<80>;
    email: string<254>;
}
```

POST routes already use these schemas to validate JSON request bodies.

## Native HTTP server preview

The `http_server` example maps a GET route to a function that returns the
response body. A `:name` segment becomes a matching `string` parameter:

```orni
func hello(name: string) -> string {
    return "Hello, " + name + "!";
}

route get "/hello/:name" => hello;
```

Run it with the native Rust target from the repository root:

```sh
cargo run -- serve examples/experimental/http_server
```

Then open `http://127.0.0.1:3000/hello/Orni`. This preview handles one
connection at a time, binds only to loopback, and supports GET, POST, PUT, and
DELETE routes. Literal routes take precedence over parameterized routes. The
WASI target remains the portable editor/compiler runtime and cannot open a
network listener.

The same example also exposes a complete in-memory user collection. The model
is the body contract, and Orni rejects missing fields, extra fields, type
mismatches, and string limits before calling the handler:

```orni
model User {
    id: uuid;
    name: string<80>;
    email: string<254>;
}

store users: User;

func create_user(input: CreateUser) -> response {
    return users.create(input);
}

func get_user(id: uuid) -> response { return users.find(id); }
func update_user(id: uuid, input: UpdateUser) -> response { return users.replace(id, input); }
func patch_user(id: uuid, input: PatchUser) -> response { return users.patch(id, input); }
func delete_user(id: uuid) -> response { return users.delete(id); }

route post "/users" => create_user;
route get "/users/:id" => get_user;
route put "/users/:id" => update_user;
route patch "/users/:id" => patch_user;
route delete "/users/:id" => delete_user;
```

Send a request while the server is running:

```sh
curl -i -X POST http://127.0.0.1:3000/users -H "Content-Type: application/json" -d '{"name":"Orni","email":"hello@orni.dev"}'
```

The server generates the UUID, responds with `201 Created`, and keeps the value until the server
stops. The remaining example handlers expose `find`, `replace`, and `delete`.

The `http_server` example also converts route values safely. Its `uint16`
handler parameter accepts `/users/42`, while an out-of-range input returns
`400 Bad Request` before the handler executes:

```orni
func user(id: uint16) -> string {
    return "User #" + id;
}

route get "/users/:id" => user;
```

Use `uuid` when identifiers are generated outside the application. Orni
validates the UUID before the handler runs, just like integer ranges.

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
