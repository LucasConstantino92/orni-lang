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
    runtime::{Interpreter, RouteProtection},
    syntax::Statement,
};

use super::{
    ExecutionLimits, MemoryStores, Route, memory_stores, request::read_request,
    response::HttpResponse, route_requires_json_body, route_response_with_stores, route_table,
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
                    );
                });
            }
            Err(error) => eprintln!("Orni HTTP connection failed: {error}"),
        }
    }
    Ok(())
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
                Some(Instant::now() + Duration::from_secs(policy.lockout_seconds));
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
        let Ok(mut entries) = self.entries.lock() else {
            return;
        };
        let now = Instant::now();
        let lifetime = Duration::from_secs(self.limits.cache_ttl_seconds);
        entries.retain(|_, entry| now.duration_since(entry.created) < lifetime);
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
fn idempotent_post_response(
    idempotency: &IdempotencyStore,
    key: &str,
    request: &super::request::HttpRequest,
    program: &[Statement],
    routes: &[Route],
    stores: MemoryStores,
    limits: ExecutionLimits,
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
    let response = route_response_with_stores(
        &request.method,
        &request.path,
        &request.body,
        program,
        routes,
        stores,
        limits,
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
) {
    let connection_timeout = Duration::from_secs(http_limits.connection_timeout_seconds);
    let _ = stream.set_read_timeout(Some(connection_timeout));
    let mut reader = BufReader::new(stream);
    for request_number in 0..http_limits.max_requests_per_connection {
        let started = Instant::now();
        let request = match read_request(&mut reader) {
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
        let response = if request.method == "GET" {
            response_cache.get(&request.path).unwrap_or_else(|| {
                let response = route_response_with_stores(
                    &request.method,
                    &request.path,
                    &request.body,
                    program,
                    routes,
                    stores.clone(),
                    limits,
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
                ),
                None => route_response_with_stores(
                    &request.method,
                    &request.path,
                    &request.body,
                    program,
                    routes,
                    stores.clone(),
                    limits,
                ),
            }
        } else {
            route_response_with_stores(
                &request.method,
                &request.path,
                &request.body,
                program,
                routes,
                stores.clone(),
                limits,
            )
        };
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
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        409 => "Conflict",
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

    use super::super::response::HttpResponse;
    use super::{
        HttpLimits, RequestLimiter, ResponseCache, RouteGuard, accepts_json, is_json_content_type,
    };
    use crate::runtime::RouteProtection;

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
    fn temporarily_locks_a_protected_route_after_failed_authentication() {
        let guard = RouteGuard::new(&[RouteProtection {
            path: "/auth/login".to_owned(),
            maximum_attempts: 3,
            lockout_seconds: 900,
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
