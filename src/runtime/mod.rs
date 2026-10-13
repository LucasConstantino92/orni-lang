//! Runtime state and evaluation for validated Orni programs.

mod environment;
mod interpreter;
mod limits;

pub(crate) use interpreter::Interpreter;
pub use limits::ExecutionLimits;
