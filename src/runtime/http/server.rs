//! Native development server and connection lifecycle.
//!
//! The server owns sockets, connection concurrency, keep-alive, and response
//! framing. Route dispatch remains isolated in the parent HTTP module.

use std::{
    collections::HashMap,
    io::{BufReader, Write},
    net::{IpAddr, TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crate::{
    HttpLimits,
    diagnostics::OrniError,
    runtime::{ApplicationConfig, AuthConfig, Interpreter, RouteProtection},
    syntax::Statement,
};

use super::auth::{self, TokenError, TokenKind};
use super::{
    ExecutionLimits, MemoryStores, Route, allowed_methods_for_path, memory_stores,
    request::read_request, response::HttpResponse, route_requires_auth, route_requires_json_body,
    route_response_with_http_context, route_table,
};

const HOST: &str = "127.0.0.1";
/// Serves the validated GET routes on the local development address.
pub(crate) fn serve(
    program: &[Statement],
    limits: ExecutionLimits,
    http_limits: HttpLimits,
) -> Result<(), OrniError> {
    let program = Arc::new(program.to_vec());
    let routes = Arc::new(route_table(&program));
    let stores = memory_stores(&program);
    if routes.is_empty() {
        return Err(
            OrniError::runtime(1, 1, "application has no HTTP routes").with_code("ORNI-E152")
        );
    }
    let (startup_output, application) = Interpreter::new(limits).execute_main(&program)?;
    let mut http_limits = http_limits;
    if let Some(limit) = application.body_limit_bytes {
        http_limits.max_request_bytes = limit;
    }
    if let Some(limit) = application.cache_limit_bytes {
        http_limits.max_cache_bytes = limit;
    }
    if (!application.authenticated_routes.is_empty() || !application.refresh_routes.is_empty())
        && application.auth.is_none()
    {
        return Err(OrniError::runtime(
            1,
            1,
            "app.require_auth and app.require_refresh require app.auth to be configured in main",
        )
        .with_code("ORNI-E152"));
    }
    print!("{startup_output}");
    let address = format!("{HOST}:{}", application.port);

    let listener = TcpListener::bind(&address).map_err(|error| {
        OrniError::runtime(
            1,
            1,
            format!("could not start HTTP server on {address}: {error}"),
        )
        .with_code("ORNI-E152")
    })?;
    let label = application.name.as_deref().unwrap_or("Orni");
    println!("{label} HTTP server listening on http://{address}");

    let connections = Arc::new(ConnectionGate::new(http_limits));
    let request_limiter = Arc::new(RequestLimiter::new(http_limits));
    let idempotency = Arc::new(IdempotencyStore::default());
    let response_cache = Arc::new(ResponseCache::new(http_limits));
    let route_guard = Arc::new(RouteGuard::new(&application.protected_routes));
    let cors = Arc::new(CorsPolicy::new(&application));
    let authentication = Arc::new(AuthenticationPolicy::new(&application));
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                let peer = stream.peer_addr().ok().map(|address| address.ip());
                let permit = match connections.acquire(peer) {
                    Ok(permit) => permit,
                    Err(ConnectionRejection::Global) => {
                        write_response(
                            &mut stream,
                            HttpResponse::error(503, "ORNI_HTTP_SERVER_BUSY", "Server is busy"),
                            true,
                        );
                        continue;
                    }
                    Err(ConnectionRejection::PerIp) => {
                        write_response(
                            &mut stream,
                            HttpResponse::error(
                                429,
                                "ORNI_HTTP_CONNECTION_LIMIT",
                                "Too many connections from this address",
                            ),
                            true,
                        );
                        continue;
                    }
                };
                let program = Arc::clone(&program);
                let routes = Arc::clone(&routes);
                let stores = stores.clone();
                let request_limiter = Arc::clone(&request_limiter);
                let idempotency = Arc::clone(&idempotency);
                let response_cache = Arc::clone(&response_cache);
                let route_guard = Arc::clone(&route_guard);
                let cors = Arc::clone(&cors);
                let authentication = Arc::clone(&authentication);
                thread::spawn(move || {
                    let _permit = permit;
                    handle_connection(
                        stream,
                        peer,
                        &program,
                        &routes,
                        stores,
                        limits,
                        http_limits,
                        request_limiter,
                        idempotency,
                        response_cache,
                        route_guard,
                        cors,
                        authentication,
                    );
                });
            }
            Err(error) => eprintln!("Orni HTTP connection failed: {error}"),
        }
    }
    Ok(())
}

/// Authentication policy captured from the application's startup configuration.
struct AuthenticationPolicy {
    configuration: Option<AuthConfig>,
    protected_routes: Vec<String>,
    refresh_routes: Vec<String>,
    logout_routes: Vec<String>,
    refresh_sessions: Mutex<RefreshSessions>,
}

impl AuthenticationPolicy {
    fn new(application: &ApplicationConfig) -> Self {
        Self {
            configuration: application.auth.clone(),
            protected_routes: application.authenticated_routes.clone(),
            refresh_routes: application.refresh_routes.clone(),
            logout_routes: application.logout_routes.clone(),
            refresh_sessions: Mutex::new(RefreshSessions::default()),
        }
    }

    fn requires_auth(&self, path: &str, routes: &[Route]) -> bool {
        route_requires_auth(path, routes, &self.protected_routes)
    }

    fn requires_refresh(&self, path: &str, routes: &[Route]) -> bool {
        route_requires_auth(path, routes, &self.refresh_routes)
    }

    fn is_logout(&self, path: &str, routes: &[Route]) -> bool {
        route_requires_auth(path, routes, &self.logout_routes)
    }

    fn consume_refresh(&self, token: &str, subject: &str) -> Result<(), HttpResponse> {
        let Ok(mut sessions) = self.refresh_sessions.lock() else {
            return Err(HttpResponse::error(
                503,
                "ORNI_HTTP_SERVER_BUSY",
                "Server is busy",
            ));
        };
        let now = Instant::now();
        sessions.prune(now, self.refresh_session_lifetime());
        if sessions.revoked_subjects.contains_key(subject) {
            return Err(HttpResponse::error(
                401,
                "ORNI_HTTP_UNAUTHORIZED",
                "Refresh session is no longer active",
            ));
        }
        let fingerprint = auth::fingerprint(token);
        if sessions.retired_tokens.contains_key(&fingerprint) {
            return Err(HttpResponse::error(
                401,
                "ORNI_HTTP_UNAUTHORIZED",
                "Refresh session is no longer active",
            ));
        }
        if sessions
            .used_tokens
            .insert(fingerprint.clone(), now)
            .is_some()
        {
            sessions.revoked_subjects.insert(subject.to_owned(), now);
            return Err(HttpResponse::error(
                401,
                "ORNI_HTTP_UNAUTHORIZED",
                "Refresh token reuse was detected",
            ));
        }
        if let Some(active) = sessions.active_by_subject.get_mut(subject) {
            active.retain(|candidate| candidate != &fingerprint);
        }
        Ok(())
    }

    /// Registers one issued refresh token and evicts only the oldest active session.
    fn register_refresh(&self, token: &str, subject: &str) -> Result<(), HttpResponse> {
        let Ok(mut sessions) = self.refresh_sessions.lock() else {
            return Err(HttpResponse::error(
                503,
                "ORNI_HTTP_SERVER_BUSY",
                "Server is busy",
            ));
        };
        let now = Instant::now();
        sessions.prune(now, self.refresh_session_lifetime());
        sessions.revoked_subjects.remove(subject);
        let fingerprint = auth::fingerprint(token);
        let maximum = self
            .configuration
            .as_ref()
            .map_or(2, |config| config.maximum_sessions_per_subject);
        let active = sessions
            .active_by_subject
            .entry(subject.to_owned())
            .or_default();
        if !active.iter().any(|candidate| candidate == &fingerprint) {
            active.push(fingerprint);
        }
        let mut retired = Vec::new();
        while active.len() > maximum {
            retired.push(active.remove(0));
        }
        sessions
            .retired_tokens
            .extend(retired.into_iter().map(|token| (token, now)));
        Ok(())
    }

    /// Tracks framework-issued refresh tokens so the per-subject session cap is enforceable.
    fn register_response_refresh(&self, response: &HttpResponse) -> Result<(), HttpResponse> {
        let Some(configuration) = &self.configuration else {
            return Ok(());
        };
        let Some(token) = response_refresh_token(response) else {
            return Ok(());
        };
        let authorization = format!("Bearer {token}");
        let token_data = auth::validate(configuration, Some(&authorization)).map_err(|_| {
            HttpResponse::error(
                500,
                "ORNI_HTTP_INTERNAL_ERROR",
                "The application returned an invalid refresh token",
            )
        })?;
        if token_data.kind != TokenKind::Refresh {
            return Err(HttpResponse::error(
                500,
                "ORNI_HTTP_INTERNAL_ERROR",
                "The application returned a token with the wrong refresh capability",
            ));
        }
        self.register_refresh(token, &token_data.subject)
    }

    fn revoke_subject(&self, subject: &str) {
        if let Ok(mut sessions) = self.refresh_sessions.lock() {
            let now = Instant::now();
            sessions.prune(now, self.refresh_session_lifetime());
            sessions.revoked_subjects.insert(subject.to_owned(), now);
            if let Some(active) = sessions.active_by_subject.remove(subject) {
                sessions
                    .retired_tokens
                    .extend(active.into_iter().map(|token| (token, now)));
            }
        }
    }

    /// Bounds replay and revocation memory to the configured refresh-token lifetime.
    fn refresh_session_lifetime(&self) -> Duration {
        Duration::from_millis(
            self.configuration
                .as_ref()
                .map_or(604_800_000, |configuration| {
                    configuration.refresh_duration_milliseconds
                }),
        )
    }

    fn subject(
        &self,
        authorization: Option<&str>,
        expected_kind: TokenKind,
    ) -> Result<Option<(String, bool)>, HttpResponse> {
        let Some(configuration) = &self.configuration else {
            return Ok(None);
        };
        auth::validate(configuration, authorization)
            .and_then(|token| {
                (token.kind == expected_kind)
                    .then_some(token)
                    .ok_or(TokenError::Invalid)
            })
            .map(|token| Some((token.subject, token.kind == TokenKind::Refresh)))
            .map_err(|error| {
                let message = match error {
                    TokenError::Missing => "A Bearer token is required",
                    TokenError::Invalid | TokenError::Expired => {
                        "Bearer token is invalid or expired"
                    }
                };
                HttpResponse::error(401, "ORNI_HTTP_UNAUTHORIZED", message)
                    .with_header("WWW-Authenticate", "Bearer")
            })
    }
}

/// In-memory refresh-token replay tracking. Persistence is added with database support.
#[derive(Default)]
struct RefreshSessions {
    used_tokens: HashMap<String, Instant>,
    revoked_subjects: HashMap<String, Instant>,
    retired_tokens: HashMap<String, Instant>,
    active_by_subject: HashMap<String, Vec<String>>,
}

impl RefreshSessions {
    /// Drops replay and revocation records once their signed token can no longer be valid.
    fn prune(&mut self, now: Instant, lifetime: Duration) {
        let is_current = |created: &Instant| now.saturating_duration_since(*created) < lifetime;
        self.used_tokens.retain(|_, created| is_current(created));
        self.revoked_subjects
            .retain(|_, created| is_current(created));
        self.retired_tokens.retain(|_, created| is_current(created));
        self.active_by_subject.retain(|_, active| {
            active.retain(|token| !self.retired_tokens.contains_key(token));
            !active.is_empty()
        });
    }
}

/// Finds a framework refresh token emitted either as a header or an HttpOnly cookie.
fn response_refresh_token(response: &HttpResponse) -> Option<&str> {
    response
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("x-refresh-token"))
        .map(|(_, value)| value.as_str())
        .or_else(|| {
            response
                .headers
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("set-cookie"))
                .and_then(|(_, value)| value.strip_prefix("orni_refresh="))
                .and_then(|value| value.split(';').next())
        })
}

/// Explicit, origin-based browser access policy configured from `app.cors`.
struct CorsPolicy {
    origins: Vec<String>,
}

impl CorsPolicy {
    fn new(application: &ApplicationConfig) -> Self {
        Self {
            origins: application.cors_origins.clone(),
        }
    }

    fn permits(&self, origin: &str) -> bool {
        self.origins.iter().any(|allowed| allowed == origin)
    }

    fn decorate(&self, response: HttpResponse, origin: Option<&str>) -> HttpResponse {
        let Some(origin) = origin.filter(|origin| self.permits(origin)) else {
            return response;
        };
        response
            .with_header("Access-Control-Allow-Origin", origin)
            .with_header("Vary", "Origin")
    }

    fn preflight(
        &self,
        origin: Option<&str>,
        path: &str,
        requested_method: Option<&str>,
        requested_headers: Option<&str>,
        routes: &[Route],
    ) -> HttpResponse {
        let Some(origin) = origin else {
            return HttpResponse::error(
                400,
                "ORNI_HTTP_INVALID_CORS_REQUEST",
                "CORS preflight requires an Origin header",
            );
        };
        if !self.permits(origin) {
            return HttpResponse::error(
                403,
                "ORNI_HTTP_CORS_ORIGIN_DENIED",
                "Origin is not allowed by this application",
            );
        }
        let Some(methods) = allowed_methods_for_path(path, routes) else {
            return HttpResponse::error(404, "ORNI_HTTP_ROUTE_NOT_FOUND", "Route not found");
        };
        let Some(requested_method) = requested_method else {
            return HttpResponse::error(
                400,
                "ORNI_HTTP_INVALID_CORS_REQUEST",
                "CORS preflight requires Access-Control-Request-Method",
            );
        };
        if !methods
            .split(", ")
            .any(|method| method.eq_ignore_ascii_case(requested_method))
        {
            return HttpResponse::error(
                405,
                "ORNI_HTTP_METHOD_NOT_ALLOWED",
                "Method is not allowed for this route",
            )
            .with_header("Allow", methods);
        }
        let headers = match allowed_cors_headers(requested_headers) {
            Ok(headers) => headers,
            Err(()) => {
                return HttpResponse::error(
                    403,
                    "ORNI_HTTP_CORS_HEADER_DENIED",
                    "Requested header is not allowed by this application",
                );
            }
        };
        let mut response = HttpResponse::text(204, "")
            .with_header("Access-Control-Allow-Origin", origin)
            .with_header("Access-Control-Allow-Methods", methods)
            .with_header(
                "Vary",
                "Origin, Access-Control-Request-Method, Access-Control-Request-Headers",
            );
        if let Some(headers) = headers {
            response = response.with_header("Access-Control-Allow-Headers", headers);
        }
        response
    }
}

/// Allows only framework-relevant browser request headers until authentication expands the set.
fn allowed_cors_headers(requested: Option<&str>) -> Result<Option<String>, ()> {
    const ALLOWED: [&str; 5] = [
        "Authorization",
        "Content-Type",
        "If-Match",
        "If-None-Match",
        "Idempotency-Key",
    ];
    let Some(requested) = requested else {
        return Ok(None);
    };
    let mut accepted = Vec::new();
    for header in requested.split(',').map(str::trim) {
        if header.is_empty() {
            return Err(());
        }
        let Some(allowed) = ALLOWED
            .iter()
            .find(|allowed| header.eq_ignore_ascii_case(allowed))
        else {
            return Err(());
        };
        if !accepted.contains(allowed) {
            accepted.push(*allowed);
        }
    }
    Ok(Some(accepted.join(", ")))
}

/// Fixed-window request limiter shared by all connections from one address.
struct RequestLimiter {
    entries: Mutex<HashMap<IpAddr, RequestWindow>>,
    limits: HttpLimits,
}

struct RequestWindow {
    started: Instant,
    requests: usize,
}

/// Short-lived POST responses retained for safe client retries.
#[derive(Default)]
struct IdempotencyStore {
    entries: Mutex<HashMap<String, IdempotencyEntry>>,
}

struct IdempotencyEntry {
    created: Instant,
    body: String,
    response: HttpResponse,
}

/// Small, bounded cache for successful public GET responses.
struct ResponseCache {
    entries: Mutex<HashMap<String, CachedResponse>>,
    limits: HttpLimits,
}

struct CachedResponse {
    created: Instant,
    bytes: usize,
    response: HttpResponse,
}

/// Failed authentication attempts tracked per protected path and remote address.
struct RouteGuard {
    policies: Vec<RouteProtection>,
    attempts: Mutex<HashMap<(String, IpAddr), FailedAttempt>>,
}

struct FailedAttempt {
    failures: usize,
    blocked_until: Option<Instant>,
}

impl RouteGuard {
    fn new(policies: &[RouteProtection]) -> Self {
        Self {
            policies: policies.to_vec(),
            attempts: Mutex::new(HashMap::new()),
        }
    }

    /// Returns the remaining lock duration for a protected route, when locked.
    fn retry_after_seconds(&self, peer: Option<IpAddr>, path: &str) -> Option<u64> {
        let (Some(peer), Some(policy)) = (peer, self.policy(path)) else {
            return None;
        };
        let Ok(mut attempts) = self.attempts.lock() else {
            return Some(1);
        };
        let key = (policy.path.clone(), peer);
        let attempt = attempts.get(&key)?;
        let now = Instant::now();
        if let Some(until) = attempt.blocked_until.filter(|until| *until > now) {
            return Some(until.duration_since(now).as_secs().max(1));
        }
        attempts.remove(&key);
        None
    }

    fn record(&self, peer: Option<IpAddr>, path: &str, status: u16) {
        let (Some(peer), Some(policy)) = (peer, self.policy(path)) else {
            return;
        };
        let Ok(mut attempts) = self.attempts.lock() else {
            return;
        };
        let key = (policy.path.clone(), peer);
        if (200..400).contains(&status) {
            attempts.remove(&key);
            return;
        }
        if !matches!(status, 401 | 403) {
            return;
        }
        let attempt = attempts.entry(key).or_insert(FailedAttempt {
            failures: 0,
            blocked_until: None,
        });
        attempt.failures += 1;
        if attempt.failures >= policy.maximum_attempts {
            attempt.blocked_until =
                Some(Instant::now() + Duration::from_millis(policy.lockout_milliseconds));
        }
    }

    fn policy(&self, request_path: &str) -> Option<&RouteProtection> {
        let path = request_path.split('?').next().unwrap_or(request_path);
        self.policies.iter().find(|policy| policy.path == path)
    }
}

impl ResponseCache {
    fn new(limits: HttpLimits) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            limits,
        }
    }

    fn get(&self, path: &str) -> Option<HttpResponse> {
        let mut entries = self.entries.lock().ok()?;
        let now = Instant::now();
        let lifetime = Duration::from_secs(self.limits.cache_ttl_seconds);
        entries.retain(|_, entry| now.duration_since(entry.created) < lifetime);
        entries.get(path).map(|entry| entry.response.clone())
    }

    fn insert(&self, path: String, response: HttpResponse) {
        if response.status != 200
            || response.headers.iter().any(|(name, value)| {
                name.eq_ignore_ascii_case("cache-control")
                    && value.to_ascii_lowercase().contains("no-store")
            })
        {
            return;
        }
        let bytes = cached_response_bytes(&path, &response);
        if bytes > self.limits.max_cache_bytes {
            return;
        }
        let Ok(mut entries) = self.entries.lock() else {
            return;
        };
        let now = Instant::now();
        let lifetime = Duration::from_secs(self.limits.cache_ttl_seconds);
        entries.retain(|_, entry| now.duration_since(entry.created) < lifetime);
        while entries
            .values()
            .map(|entry| entry.bytes)
            .sum::<usize>()
            .saturating_add(bytes)
            > self.limits.max_cache_bytes
        {
            let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, entry)| entry.created)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            entries.remove(&oldest);
        }
        if !entries.contains_key(&path)
            && entries.len() >= self.limits.max_cache_entries
            && let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, entry)| entry.created)
                .map(|(key, _)| key.clone())
        {
            entries.remove(&oldest);
        }
        entries.insert(
            path,
            CachedResponse {
                created: now,
                bytes,
                response,
            },
        );
    }

    fn invalidate(&self) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.clear();
        }
    }
}

/// Estimates the retained UTF-8 bytes for one cached response and its lookup key.
fn cached_response_bytes(path: &str, response: &HttpResponse) -> usize {
    path.len()
        .saturating_add(response.body.len())
        .saturating_add(response.content_type.len())
        .saturating_add(
            response
                .headers
                .iter()
                .map(|(name, value)| name.len().saturating_add(value.len()))
                .sum::<usize>(),
        )
}

impl RequestLimiter {
    fn new(limits: HttpLimits) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            limits,
        }
    }

    fn allow(&self, ip: Option<IpAddr>) -> bool {
        let Some(ip) = ip else {
            return true;
        };
        let Ok(mut entries) = self.entries.lock() else {
            return false;
        };
        let now = Instant::now();
        let window = Duration::from_secs(self.limits.request_window_seconds);
        entries.retain(|_, entry| now.duration_since(entry.started) < window);
        let maximum_entries = self.limits.max_connections.saturating_mul(16).max(1);
        if !entries.contains_key(&ip) && entries.len() >= maximum_entries {
            return false;
        }
        let entry = entries.entry(ip).or_insert(RequestWindow {
            started: now,
            requests: 0,
        });
        if entry.requests >= self.limits.max_requests_per_ip {
            return false;
        }
        entry.requests += 1;
        true
    }
}

/// Replays a completed POST when its route, key, and body match a recent request.
///
/// The mutex deliberately stays held through the first execution. That makes two
/// simultaneous retries with the same key one creation, rather than a race that
/// merely looks idempotent after the fact.
#[allow(clippy::too_many_arguments)]
fn idempotent_post_response(
    idempotency: &IdempotencyStore,
    key: &str,
    request: &super::request::HttpRequest,
    program: &[Statement],
    routes: &[Route],
    stores: MemoryStores,
    limits: ExecutionLimits,
    auth: Option<AuthConfig>,
    authenticated_subject: Option<String>,
    authenticated_with_refresh_token: bool,
) -> HttpResponse {
    let Ok(mut entries) = idempotency.entries.lock() else {
        return HttpResponse::error(503, "ORNI_HTTP_SERVER_BUSY", "Server is busy");
    };
    let now = Instant::now();
    let lifetime = Duration::from_secs(300);
    entries.retain(|_, entry| now.duration_since(entry.created) < lifetime);
    let cache_key = format!("{} {} {key}", request.method, request.path);
    if let Some(entry) = entries.get(&cache_key) {
        if entry.body == request.body {
            return entry.response.clone();
        }
        return HttpResponse::error(
            409,
            "ORNI_HTTP_IDEMPOTENCY_CONFLICT",
            "Idempotency-Key was already used with a different request body",
        );
    }
    let response = route_response_with_http_context(
        &request.method,
        &request.path,
        &request.body,
        program,
        routes,
        stores,
        limits,
        None,
        auth,
        authenticated_subject,
        authenticated_with_refresh_token,
    );
    if response.status < 500 {
        entries.insert(
            cache_key,
            IdempotencyEntry {
                created: now,
                body: request.body.clone(),
                response: response.clone(),
            },
        );
    }
    response
}

/// Shared global and per-address admission control for TCP connections.
struct ConnectionGate {
    active: AtomicUsize,
    per_ip: Mutex<HashMap<IpAddr, usize>>,
    limits: HttpLimits,
}

impl ConnectionGate {
    fn new(limits: HttpLimits) -> Self {
        Self {
            active: AtomicUsize::new(0),
            per_ip: Mutex::new(HashMap::new()),
            limits,
        }
    }

    fn acquire(
        self: &Arc<Self>,
        ip: Option<IpAddr>,
    ) -> Result<ConnectionPermit, ConnectionRejection> {
        self.active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                (active < self.limits.max_connections).then_some(active + 1)
            })
            .map_err(|_| ConnectionRejection::Global)?;
        if let Some(ip) = ip {
            let mut per_ip = match self.per_ip.lock() {
                Ok(per_ip) => per_ip,
                Err(_) => {
                    self.active.fetch_sub(1, Ordering::Release);
                    return Err(ConnectionRejection::Global);
                }
            };
            let active = per_ip.entry(ip).or_default();
            if *active >= self.limits.max_connections_per_ip {
                self.active.fetch_sub(1, Ordering::Release);
                return Err(ConnectionRejection::PerIp);
            }
            *active += 1;
        }
        Ok(ConnectionPermit {
            gate: Arc::clone(self),
            ip,
        })
    }
}

enum ConnectionRejection {
    Global,
    PerIp,
}

/// Releases one connection permit when its worker exits.
struct ConnectionPermit {
    gate: Arc<ConnectionGate>,
    ip: Option<IpAddr>,
}

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        self.gate.active.fetch_sub(1, Ordering::Release);
        if let Some(ip) = self.ip
            && let Ok(mut per_ip) = self.gate.per_ip.lock()
            && let Some(active) = per_ip.get_mut(&ip)
        {
            *active -= 1;
            if *active == 0 {
                per_ip.remove(&ip);
            }
        }
    }
}

/// Handles a bounded sequence of requests on one HTTP/1.1 connection.
#[allow(clippy::too_many_arguments)]
fn handle_connection(
    stream: TcpStream,
    peer: Option<IpAddr>,
    program: &[Statement],
    routes: &[Route],
    stores: MemoryStores,
    limits: ExecutionLimits,
    http_limits: HttpLimits,
    request_limiter: Arc<RequestLimiter>,
    idempotency: Arc<IdempotencyStore>,
    response_cache: Arc<ResponseCache>,
    route_guard: Arc<RouteGuard>,
    cors: Arc<CorsPolicy>,
    authentication: Arc<AuthenticationPolicy>,
) {
    let connection_timeout = Duration::from_secs(http_limits.connection_timeout_seconds);
    let _ = stream.set_read_timeout(Some(connection_timeout));
    let mut reader = BufReader::new(stream);
    for request_number in 0..http_limits.max_requests_per_connection {
        let started = Instant::now();
        let request = match read_request(&mut reader, http_limits.max_request_bytes) {
            Ok(Some(request)) => request,
            Ok(None) => return,
            Err(message) => {
                write_response(
                    reader.get_mut(),
                    HttpResponse::error(message.status, message.code, message.message),
                    true,
                );
                return;
            }
        };
        if !request_limiter.allow(peer) {
            write_response(
                reader.get_mut(),
                HttpResponse::error(
                    429,
                    "ORNI_HTTP_RATE_LIMITED",
                    "Too many requests from this address",
                ),
                true,
            );
            return;
        }
        if request.method == "OPTIONS"
            && (request.origin.is_some() || request.access_control_request_method.is_some())
        {
            let response = cors.preflight(
                request.origin.as_deref(),
                &request.path,
                request.access_control_request_method.as_deref(),
                request.access_control_request_headers.as_deref(),
                routes,
            );
            let close_connection = request.close_connection
                || started.elapsed() >= connection_timeout
                || request_number + 1 == http_limits.max_requests_per_connection;
            write_response(reader.get_mut(), response, close_connection);
            if close_connection {
                return;
            }
            continue;
        }
        if let Some(retry_after) = route_guard.retry_after_seconds(peer, &request.path) {
            write_response(
                reader.get_mut(),
                HttpResponse::error(
                    429,
                    "ORNI_HTTP_PROTECTED_ROUTE_LOCKED",
                    "Too many failed authentication attempts. Try again later.",
                )
                .with_header("Retry-After", retry_after.to_string()),
                true,
            );
            return;
        }
        let requires_authentication = authentication.requires_auth(&request.path, routes);
        let requires_refresh = authentication.requires_refresh(&request.path, routes);
        let cookie_authorization = requires_refresh
            .then(|| {
                request
                    .refresh_cookie
                    .as_ref()
                    .map(|token| format!("Bearer {token}"))
            })
            .flatten();
        let authorization = request
            .authorization
            .as_deref()
            .or(cookie_authorization.as_deref());
        let (authenticated_subject, authenticated_with_refresh_token) =
            if requires_authentication || requires_refresh {
                let expected_kind = if requires_refresh {
                    TokenKind::Refresh
                } else {
                    TokenKind::Access
                };
                match authentication.subject(authorization, expected_kind) {
                    Ok(Some((subject, is_refresh))) => (Some(subject), is_refresh),
                    Ok(None) => (None, false),
                    Err(response) => {
                        let response = cors.decorate(response, request.origin.as_deref());
                        write_response(reader.get_mut(), response, request.close_connection);
                        if request.close_connection {
                            return;
                        }
                        continue;
                    }
                }
            } else {
                (None, false)
            };
        if requires_refresh {
            let token = authorization
                .and_then(|value| value.split_once(' ').map(|(_, token)| token))
                .unwrap_or_default();
            if let Some(subject) = authenticated_subject.as_deref()
                && let Err(response) = authentication.consume_refresh(token, subject)
            {
                let response = cors.decorate(response, request.origin.as_deref());
                write_response(reader.get_mut(), response, request.close_connection);
                if request.close_connection {
                    return;
                }
                continue;
            }
        }
        if route_requires_json_body(&request.method, &request.path, routes)
            && !is_json_content_type(request.content_type.as_deref())
        {
            write_response(
                reader.get_mut(),
                HttpResponse::error(
                    415,
                    "ORNI_HTTP_UNSUPPORTED_MEDIA_TYPE",
                    "This route requires Content-Type: application/json",
                ),
                request.close_connection,
            );
            if request.close_connection {
                return;
            }
            continue;
        }
        if route_requires_json_body(&request.method, &request.path, routes)
            && !accepts_json(request.accept.as_deref())
        {
            write_response(
                reader.get_mut(),
                HttpResponse::error(
                    406,
                    "ORNI_HTTP_NOT_ACCEPTABLE",
                    "This route produces application/json, which the request does not accept",
                ),
                request.close_connection,
            );
            if request.close_connection {
                return;
            }
            continue;
        }
        let mut response = if request.method == "GET" && !requires_authentication {
            response_cache.get(&request.path).unwrap_or_else(|| {
                let response = route_response_with_http_context(
                    &request.method,
                    &request.path,
                    &request.body,
                    program,
                    routes,
                    stores.clone(),
                    limits,
                    None,
                    authentication.configuration.clone(),
                    authenticated_subject.clone(),
                    authenticated_with_refresh_token,
                );
                response_cache.insert(request.path.clone(), response.clone());
                response
            })
        } else if request.method == "POST" {
            match request.idempotency_key.as_deref() {
                Some(key) => idempotent_post_response(
                    &idempotency,
                    key,
                    &request,
                    program,
                    routes,
                    stores.clone(),
                    limits,
                    authentication.configuration.clone(),
                    authenticated_subject.clone(),
                    authenticated_with_refresh_token,
                ),
                None => route_response_with_http_context(
                    &request.method,
                    &request.path,
                    &request.body,
                    program,
                    routes,
                    stores.clone(),
                    limits,
                    None,
                    authentication.configuration.clone(),
                    authenticated_subject.clone(),
                    authenticated_with_refresh_token,
                ),
            }
        } else {
            let expected_store_version = match request.if_match.as_deref() {
                Some(value) => match parse_if_match(value) {
                    Ok(version) => Some(version),
                    Err(message) => {
                        write_response(
                            reader.get_mut(),
                            HttpResponse::error(400, "ORNI_HTTP_INVALID_IF_MATCH", message),
                            request.close_connection,
                        );
                        if request.close_connection {
                            return;
                        }
                        continue;
                    }
                },
                None => None,
            };
            route_response_with_http_context(
                &request.method,
                &request.path,
                &request.body,
                program,
                routes,
                stores.clone(),
                limits,
                expected_store_version,
                authentication.configuration.clone(),
                authenticated_subject.clone(),
                authenticated_with_refresh_token,
            )
        };
        if request.method == "GET" {
            response = response.not_modified_if_matches(request.if_none_match.as_deref());
        }
        response = cors.decorate(response, request.origin.as_deref());
        if requires_refresh && response.status < 400 {
            if authentication.is_logout(&request.path, routes) {
                if let Some(subject) = authenticated_subject.as_deref() {
                    authentication.revoke_subject(subject);
                }
            } else if let (Some(configuration), Some(subject)) = (
                authentication.configuration.as_ref(),
                authenticated_subject.as_deref(),
            ) {
                let refresh = auth::issue_refresh(configuration, subject);
                if let Err(error) = authentication.register_refresh(&refresh, subject) {
                    response = error;
                } else {
                    let max_age = configuration.refresh_duration_milliseconds / 1_000;
                    response = response
                        .with_header("X-Refresh-Token", refresh.clone())
                        .with_header(
                            "Set-Cookie",
                            format!(
                                "orni_refresh={refresh}; Max-Age={max_age}; HttpOnly; Secure; SameSite=Strict; Path=/auth"
                            ),
                        );
                }
            }
        }
        if response.status < 400 {
            if let Err(error) = authentication.register_response_refresh(&response) {
                response = error;
            }
        }
        if request.method != "GET" && response.status < 400 {
            response_cache.invalidate();
        }
        route_guard.record(peer, &request.path, response.status);
        let close_connection = request.close_connection
            || started.elapsed() >= connection_timeout
            || request_number + 1 == http_limits.max_requests_per_connection;
        let status = response.status;
        write_response(reader.get_mut(), response, close_connection);
        if request_logging_enabled() {
            println!(
                "{} {} -> {} ({}ms)",
                request.method,
                request.path,
                status,
                started.elapsed().as_millis()
            );
        }
        if close_connection {
            return;
        }
    }
}

/// Parses the single strong entity-tag format emitted by Orni resource routes.
fn parse_if_match(value: &str) -> Result<u64, &'static str> {
    let Some(version) = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    else {
        return Err("If-Match must be an ETag returned by this API, such as \"1\"");
    };
    if version.is_empty() || !version.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("If-Match must be an ETag returned by this API, such as \"1\"");
    }
    version
        .parse()
        .map_err(|_| "If-Match contains an unsupported resource revision")
}

/// Accepts JSON media types with ordinary optional parameters such as charset.
fn is_json_content_type(content_type: Option<&str>) -> bool {
    let Some(content_type) = content_type else {
        return false;
    };
    let media_type = content_type
        .split_once(';')
        .map_or(content_type, |(media_type, _)| media_type)
        .trim()
        .to_ascii_lowercase();
    media_type == "application/json"
        || media_type.starts_with("application/") && media_type.ends_with("+json")
}

/// Treats an omitted Accept header as the HTTP default: every representation is acceptable.
fn accepts_json(accept: Option<&str>) -> bool {
    let Some(accept) = accept else {
        return true;
    };
    accept.split(',').any(|entry| {
        let mut pieces = entry.split(';');
        let media_type = pieces
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        let allowed = pieces.all(|parameter| {
            let parameter = parameter.trim();
            !parameter
                .strip_prefix("q=")
                .is_some_and(|quality| quality.trim() == "0" || quality.trim() == "0.0")
        });
        allowed
            && (matches!(
                media_type.as_str(),
                "*/*" | "application/*" | "application/json"
            ) || media_type.starts_with("application/") && media_type.ends_with("+json"))
    })
}

/// Enables development request logs only when explicitly requested.
fn request_logging_enabled() -> bool {
    std::env::var("ORNI_HTTP_LOG")
        .is_ok_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE"))
}

/// Serializes one response and explicitly signals the connection lifetime.
fn write_response(stream: &mut TcpStream, response: HttpResponse, close_connection: bool) {
    let reason = match response.status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        409 => "Conflict",
        412 => "Precondition Failed",
        415 => "Unsupported Media Type",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        503 => "Service Unavailable",
        _ => "Internal Server Error",
    };
    let headers = response
        .headers
        .iter()
        .map(|(name, value)| format!("{name}: {value}\r\n"))
        .collect::<String>();
    let wire_response = format!(
        "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\n{headers}Connection: {}\r\n\r\n{}",
        response.status,
        response.content_type,
        response.body.len(),
        if close_connection {
            "close"
        } else {
            "keep-alive"
        },
        response.body,
    );
    if let Err(error) = stream.write_all(wire_response.as_bytes()) {
        eprintln!("Orni HTTP response write failed: {error}");
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::super::{auth, response::HttpResponse, route_table};
    use super::{
        AuthenticationPolicy, CorsPolicy, HttpLimits, RequestLimiter, ResponseCache, RouteGuard,
        accepts_json, allowed_cors_headers, is_json_content_type, parse_if_match,
    };
    use crate::{
        compile,
        runtime::{ApplicationConfig, AuthConfig, RouteProtection},
    };

    #[test]
    fn accepts_json_media_types_with_optional_parameters() {
        assert!(is_json_content_type(Some("application/json")));
        assert!(is_json_content_type(Some(
            "Application/Json; charset=utf-8"
        )));
        assert!(is_json_content_type(Some("application/problem+json")));
        assert!(!is_json_content_type(None));
        assert!(!is_json_content_type(Some("text/plain")));
    }

    #[test]
    fn respects_explicit_json_acceptance() {
        assert!(accepts_json(None));
        assert!(accepts_json(Some("application/json")));
        assert!(accepts_json(Some("text/plain, application/problem+json")));
        assert!(accepts_json(Some("application/*; q=0.8")));
        assert!(!accepts_json(Some("text/plain")));
        assert!(!accepts_json(Some("application/json; q=0")));
    }

    #[test]
    fn accepts_only_the_entity_tag_format_emitted_for_store_resources() {
        assert_eq!(parse_if_match("\"42\""), Ok(42));
        assert!(parse_if_match("42").is_err());
        assert!(parse_if_match("W/\"42\"").is_err());
        assert!(parse_if_match("\"not-a-version\"").is_err());
    }

    #[test]
    fn returns_not_modified_only_for_a_matching_successful_entity_tag() {
        let tagged = HttpResponse::text(200, "resource").with_header("ETag", "\"2\"");
        let unchanged = tagged.clone().not_modified_if_matches(Some("\"1\", \"2\""));
        assert_eq!(unchanged.status, 304);
        assert!(unchanged.body.is_empty());
        assert_eq!(
            unchanged.headers,
            vec![("ETag".to_owned(), "\"2\"".to_owned())]
        );

        let changed = tagged.not_modified_if_matches(Some("\"1\""));
        assert_eq!(changed.status, 200);
        assert_eq!(changed.body, "resource");
    }

    #[test]
    fn cors_allows_only_declared_origins_and_known_preflight_headers() {
        let policy = CorsPolicy::new(&ApplicationConfig {
            cors_origins: vec!["http://localhost:5173".to_owned()],
            ..ApplicationConfig::default()
        });
        let program = compile(
            r#"
                func users() -> string { return "users"; }
                route get "/users" => users;
            "#,
        )
        .expect("CORS test routes should compile");
        let routes = route_table(&program);
        let response = policy.preflight(
            Some("http://localhost:5173"),
            "/users",
            Some("GET"),
            Some("content-type, if-none-match"),
            &routes,
        );
        assert_eq!(response.status, 204);
        assert!(response.headers.contains(&(
            "Access-Control-Allow-Origin".to_owned(),
            "http://localhost:5173".to_owned(),
        )));
        assert!(response.headers.contains(&(
            "Access-Control-Allow-Headers".to_owned(),
            "Content-Type, If-None-Match".to_owned(),
        )));
        assert_eq!(
            policy
                .preflight(
                    Some("https://untrusted.example"),
                    "/users",
                    Some("GET"),
                    None,
                    &routes,
                )
                .status,
            403
        );
        assert_eq!(
            allowed_cors_headers(Some("Authorization")),
            Ok(Some("Authorization".to_owned()))
        );
    }

    #[test]
    fn limits_requests_from_one_address_within_the_window() {
        let limiter = RequestLimiter::new(HttpLimits {
            max_requests_per_ip: 2,
            ..HttpLimits::default()
        });
        let address = Some(IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert!(limiter.allow(address));
        assert!(limiter.allow(address));
        assert!(!limiter.allow(address));
    }

    #[test]
    fn caches_successful_get_responses_and_drops_them_on_invalidation() {
        let cache = ResponseCache::new(HttpLimits::default());
        cache.insert(
            "/users?search=orni".to_owned(),
            HttpResponse::text(200, "first"),
        );
        assert_eq!(
            cache
                .get("/users?search=orni")
                .map(|response| response.body),
            Some("first".to_owned())
        );

        cache.insert(
            "/health".to_owned(),
            HttpResponse::text(200, "ready").with_header("Cache-Control", "no-store"),
        );
        assert!(cache.get("/health").is_none());

        cache.invalidate();
        assert!(cache.get("/users?search=orni").is_none());
    }

    #[test]
    fn bounds_cached_responses_by_the_declared_memory_budget() {
        let cache = ResponseCache::new(HttpLimits {
            max_cache_bytes: 100,
            ..HttpLimits::default()
        });
        cache.insert(
            "/first".to_owned(),
            HttpResponse::text(200, "1234567890123456789012345678901234567890"),
        );
        cache.insert(
            "/second".to_owned(),
            HttpResponse::text(200, "1234567890123456789012345678901234567890"),
        );
        assert!(cache.get("/first").is_none());
        assert!(cache.get("/second").is_some());
    }

    #[test]
    fn keeps_only_the_configured_number_of_refresh_sessions() {
        let configuration = AuthConfig {
            secret: "a development secret with enough entropy".to_owned(),
            access_duration_milliseconds: 3_600_000,
            refresh_duration_milliseconds: 604_800_000,
            maximum_sessions_per_subject: 2,
        };
        let policy = AuthenticationPolicy::new(&ApplicationConfig {
            auth: Some(configuration.clone()),
            ..ApplicationConfig::default()
        });
        for _ in 0..3 {
            let token = auth::issue_refresh(&configuration, "user-42");
            policy
                .register_refresh(&token, "user-42")
                .expect("token tracking should be available");
        }
        let sessions = policy
            .refresh_sessions
            .lock()
            .expect("session tracking lock should be available");
        assert_eq!(sessions.active_by_subject["user-42"].len(), 2);
        assert_eq!(sessions.retired_tokens.len(), 1);
    }

    #[test]
    fn temporarily_locks_a_protected_route_after_failed_authentication() {
        let guard = RouteGuard::new(&[RouteProtection {
            path: "/auth/login".to_owned(),
            maximum_attempts: 3,
            lockout_milliseconds: 900_000,
        }]);
        let peer = Some(IpAddr::V4(Ipv4Addr::LOCALHOST));
        guard.record(peer, "/auth/login", 401);
        guard.record(peer, "/auth/login", 401);
        assert!(guard.retry_after_seconds(peer, "/auth/login").is_none());
        guard.record(peer, "/auth/login", 200);
        guard.record(peer, "/auth/login", 401);
        guard.record(peer, "/auth/login", 401);
        guard.record(peer, "/auth/login", 401);
        assert!(guard.retry_after_seconds(peer, "/auth/login").is_some());
        assert!(guard.retry_after_seconds(peer, "/users").is_none());
    }
}
