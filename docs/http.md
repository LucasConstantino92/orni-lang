# Orni HTTP guide

[← Back to Orni](../README.md) · [Language guide](language.md) · [Authentication guide](authentication.md)

## Application entry point

`main` is the application heart. It configures the server once while routes and
features remain in their own modules.

```orni
func main() -> unit {
    app.name("My SaaS");
    app.port(3000);
    app.body_limit(Size(megabytes: 2));
    app.cache_limit(Size(megabytes: 16));
}
```

## CRUD routes

```orni
model User {
    id: uuid;
    name: string<80>;
    email: string<254>, email, unique;
}

model CreateUser {
    name: string<80>;
    email: string<254>, email;
}

store users: User;

func create_user(input: CreateUser) -> response {
    return users.create(input);
}

func list_users() -> response { return users.list(); }
func get_user(id: uuid) -> response { return users.find(id); }

route post "/users" => create_user;
route get "/users" => list_users;
route get "/users/:id" => get_user;
```

Stores are intentionally in memory today. They are useful for prototypes and
language validation, but are not a database replacement.

## Collections, cache, and concurrent updates

Use `store.list(limit, cursor)` or `store.search(...)` to expose bounded
collection windows instead of loading every record. Successful public GET
responses can be cached briefly; writes invalidate the cache. `app.cache_limit`
puts an upper memory boundary around that cache.

Model-backed resources include ETags. Send `If-Match` for PUT, PATCH, or DELETE
when a client must avoid overwriting a newer representation. A stale mutation
returns `412 Precondition Failed`.

## HTTP boundaries and CORS

Project-wide defaults can be customized in `orni.toml`:

```toml
[http]
connection_timeout_seconds = 5
max_connections = 256
max_connections_per_ip = 32
max_requests_per_ip = 120
request_window_seconds = 60
max_request_bytes = 1048576
max_cache_bytes = 16777216
```

Orni validates request framing, request body size, JSON media types, and common
HTTP preconditions before executing a handler. Oversized bodies receive `413`;
oversized headers receive `431`.

Browser access is closed by default. Allow each frontend origin explicitly:

```orni
func main() -> unit {
    app.cors("http://localhost:5173");
    app.cors("https://app.example.com");
}
```

For sensitive routes, use the built-in per-IP guard:

```orni
app.protect(
    route: "/auth/login",
    attempts: 5,
    lock_time: Duration(minutes: 15)
);
```

See [authentication.md](authentication.md) for token and session handling.
