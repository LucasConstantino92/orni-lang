//! Runtime state and evaluation for validated Orni programs.

pub(crate) mod core;
mod environment;
#[cfg(not(target_arch = "wasm32"))]
mod http;
mod interpreter;
mod limits;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct MemoryStore {
    pub(crate) model: String,
    pub(crate) fields: Vec<crate::syntax::ModelField>,
    pub(crate) values: std::collections::BTreeMap<uuid::Uuid, crate::syntax::Value>,
    /// Monotonically increasing revisions used by HTTP ETag preconditions.
    pub(crate) versions: std::collections::BTreeMap<uuid::Uuid, u64>,
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) type MemoryStores =
    std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, MemoryStore>>>;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use http::serve;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use http::{ApiTestContext, api_test_context, api_test_request};
pub(crate) use interpreter::Interpreter;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use interpreter::{ApplicationConfig, AuthConfig, RouteProtection};
pub use limits::ExecutionLimits;

/// Native HTTP server boundaries that protect local application availability.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HttpLimits {
    /// Idle read timeout applied to each accepted TCP connection, in seconds.
    pub connection_timeout_seconds: u64,
    /// Maximum number of simultaneous TCP connections across the server.
    pub max_connections: usize,
    /// Maximum simultaneous TCP connections accepted from one IP address.
    pub max_connections_per_ip: usize,
    /// Maximum HTTP requests served before a keep-alive connection is closed.
    pub max_requests_per_connection: usize,
    /// Maximum requests accepted from one IP address during one rate-limit window.
    pub max_requests_per_ip: usize,
    /// Fixed request-rate window duration, in seconds.
    pub request_window_seconds: u64,
    /// Lifetime of one cached successful GET response, in seconds.
    pub cache_ttl_seconds: u64,
    /// Maximum retained successful GET responses.
    pub max_cache_entries: usize,
    /// Maximum accepted HTTP request body size, in bytes.
    pub max_request_bytes: usize,
    /// Maximum combined retained GET response size, in bytes.
    pub max_cache_bytes: usize,
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for HttpLimits {
    fn default() -> Self {
        Self {
            connection_timeout_seconds: 5,
            max_connections: 256,
            max_connections_per_ip: 32,
            max_requests_per_connection: 1_000,
            max_requests_per_ip: 120,
            request_window_seconds: 60,
            cache_ttl_seconds: 15,
            max_cache_entries: 256,
            max_request_bytes: 1_048_576,
            max_cache_bytes: 16 * 1_048_576,
        }
    }
}
