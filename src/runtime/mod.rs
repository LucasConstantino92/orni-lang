//! Runtime state and evaluation for validated Orni programs.

mod environment;
#[cfg(not(target_arch = "wasm32"))]
mod http;
mod interpreter;
mod limits;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct MemoryStore {
    pub(crate) model: String,
    pub(crate) values: std::collections::BTreeMap<uuid::Uuid, crate::syntax::Value>,
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) type MemoryStores =
    std::rc::Rc<std::cell::RefCell<std::collections::HashMap<String, MemoryStore>>>;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use http::serve;
pub(crate) use interpreter::Interpreter;
pub use limits::ExecutionLimits;
