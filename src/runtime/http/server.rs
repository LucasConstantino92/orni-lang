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

use crate::{HttpLimits, diagnostics::OrniError, runtime::Interpreter, syntax::Statement};

use super::{
    ExecutionLimits, MemoryStores, Route, memory_stores, request::read_request,
    response::HttpResponse, route_response_with_stores, route_table,
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
        let response = route_response_with_stores(
            &request.method,
            &request.path,
            &request.body,
            program,
            routes,
            stores.clone(),
            limits,
        );
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
        409 => "Conflict",
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

    use super::{HttpLimits, RequestLimiter};

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
}
