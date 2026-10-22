# Orni language guide

[← Back to Orni](../README.md) · [HTTP guide](http.md) · [Editor guide](editor.md)

## Types and inference

Orni infers a compact string capacity and the smallest safe integer type when
the value is known. Add an explicit type when a public contract needs a stable
boundary.

```orni
let name = "Orni";            // string<4>
let port = 8080;               // int16
let email: string<254> = "hello@example.com";
let enabled: bool = true;
```

`string<N>` measures UTF-8 bytes. Arithmetic is checked, and invalid narrowing,
overflow, and division by zero produce diagnostics instead of changing a value
silently.

| Type | Purpose |
| --- | --- |
| `string` / `string<N>` | UTF-8 text with inferred or explicit capacity |
| `int`, `int8`–`int64` | Checked signed integers |
| `uint8`–`uint64` | Checked unsigned integers |
| `double`, `bool`, `char`, `uuid` | Fractional, logical, Unicode, and UUID values |
| `size` / `Size(...)` | Explicit byte limits |
| `duration` / `Duration(...)` | Explicit elapsed time values |
| `response`, `unit` | HTTP results and no reusable value |

## Functions and visibility

```orni
/// Returns a friendly greeting.
func greet(name: string, greeting: string = "Hello") -> string {
    return greeting + ", " + name;
}

func _format_internal(value: string) -> string {
    return value;
}
```

Functions, parameters, defaults, and return values are typed. A leading
underscore marks an internal declaration; public declarations are available to
other modules.

## Models and validation

Models define typed JSON boundaries and in-memory store records.

```orni
model CreateUser {
    name: string<80>, min(2), max(80);
    email: string<254>, email, unique;
    age: uint8, min(18), max(120);
    role: string<16>, one_of("member", "admin") = "member";
    bio: string<500>, optional;
}
```

Rules sit beside each field. Orni validates incoming JSON before a route handler
runs and returns field-specific `400` details. `unique` is enforced by an
in-memory store; a production database adapter is planned for a later release.

## Modules and dependency injection

Use file modules to keep application features small. Imports expose only public
members, and the editor reports circular or ambiguous imports.

```orni
import services.greeting.{greet};
import contracts.clock as clock;
```

Contracts and providers provide explicit dependency injection. Bind one provider
to a contract in the application entry point, then request the contract from a
provider or function that needs it.

See the runnable examples in [`examples/experimental`](../examples/experimental).
