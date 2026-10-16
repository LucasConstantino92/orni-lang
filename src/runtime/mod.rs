//! Runtime state and evaluation for validated Orni programs.

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
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) type MemoryStores =
    std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, MemoryStore>>>;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use http::serve;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use http::{ApiTestContext, api_test_context, api_test_request};
pub(crate) use interpreter::Interpreter;
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
        }
    }
}
