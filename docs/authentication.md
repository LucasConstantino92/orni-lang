# Orni authentication guide

[← Back to Orni](../README.md) · [HTTP guide](http.md)

Orni provides signed short-lived access tokens and rotating refresh tokens at
the HTTP boundary. Your application remains responsible for verifying
credentials and storing password hashes.

```orni
func main() -> unit {
    app.auth(
        secret: "replace-this-development-secret",
        access_time: Duration(hours: 1),
        refresh_time: Duration(days: 7),
        max_sessions: 2
    );
    app.require_auth("/profile");
    app.require_refresh("/auth/refresh");
    app.logout("/auth/logout");
}
```

## Login, refresh, and logout

```orni
func login(input: LoginInput) -> response {
    let refresh = auth.refresh_token("user-42");
    return auth.cookie(
        response.header(response.ok(auth.token("user-42")), "X-Refresh-Token", refresh),
        refresh
    );
}

func profile() -> response { return response.ok(auth.user()); }
func refresh(input: LoginInput) -> response { return response.ok(auth.refresh()); }
func logout(input: LoginInput) -> response { return response.no_content(); }
```

Refresh succeeds only on a route declared with `app.require_refresh`. It rotates
the token and emits a replacement through `X-Refresh-Token` and an `HttpOnly`,
`Secure`, `SameSite=Strict` cookie. Reusing an older refresh token revokes the
subject's active in-memory sessions.

`max_sessions` defaults to two. When another session is issued, Orni retires
the oldest active refresh token for that subject.

## Important current boundary

Refresh tracking, logout revocation, route lockouts, and request limits are
in-process state. They reset when the server restarts and do not synchronize
between server instances. Use this foundation for local development and early
prototypes; production multi-instance deployments need durable shared storage
for users, sessions, and rate limits.

Use HTTPS in browsers. The secure refresh cookie deliberately is not sent over
plain HTTP; local command-line tests can pass it explicitly.
