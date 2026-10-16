//! The Orni language runtime.
//!
//! This crate owns the complete source-to-execution pipeline. Source text is
//! tokenized by the lexer, transformed into an abstract syntax tree by the
//! parser, validated by the type checker, and finally evaluated by the
//! interpreter.

#![warn(missing_docs)]

mod diagnostics;
mod frontend;
mod pipeline;
mod project;
#[cfg(test)]
mod regression_tests;
mod runtime;
mod syntax;

pub use diagnostics::{DiagnosticSeverity, OrniDiagnostic, OrniError};
pub use pipeline::{
    check, check_file, check_module, diagnose_file, diagnose_file_with_source, diagnose_module,
    diagnose_module_buffers, diagnose_project_buffers, run, run_file, run_file_with_limits,
    run_with_limits,
};
#[cfg(test)]
pub(crate) use pipeline::{compile, compile_file};
#[cfg(not(target_arch = "wasm32"))]
pub use pipeline::{run_api_test_file, serve_file};
pub use project::Project;
pub use runtime::ExecutionLimits;
#[cfg(not(target_arch = "wasm32"))]
pub use runtime::HttpLimits;
#[cfg(test)]
pub(crate) use runtime::Interpreter;

#[cfg(test)]
mod tests;
