//! Public compilation and execution pipeline.
//!
//! This module is the stable boundary between Orni's public API and its
//! internal compiler and runtime layers. Keeping this orchestration here lets
//! the crate root remain a small, discoverable facade.

use std::path::Path;

use crate::{
    ExecutionLimits, OrniDiagnostic, OrniError, Project,
    frontend::{Lexer, ModuleLoader, Parser, TypeChecker},
    runtime::Interpreter,
};

/// Checks an Orni program without executing it.
///
/// This performs lexical, syntax, and semantic validation. Editors can use
/// it to report diagnostics without triggering program behavior.
pub fn check(source: &str) -> Result<(), OrniError> {
    compile(source).map(|_| ())
}

/// Checks an Orni entry file and all modules imported by it.
pub fn check_file(path: impl AsRef<Path>) -> Result<(), OrniError> {
    compile_file(path.as_ref()).map(|_| ())
}

/// Checks a file-based program and returns every non-fatal compiler diagnostic.
pub fn diagnose_file(path: impl AsRef<Path>) -> Result<Vec<OrniDiagnostic>, OrniError> {
    let (program, mut diagnostics) = ModuleLoader::new().load_with_diagnostics(path.as_ref())?;
    diagnostics.extend(TypeChecker::new().check_with_diagnostics(&program)?);
    Ok(diagnostics)
}

/// Checks an application using one unsaved editor buffer in its module graph.
pub fn diagnose_file_with_source(
    entry: impl AsRef<Path>,
    file: impl AsRef<Path>,
    source: &str,
) -> Result<Vec<OrniDiagnostic>, OrniError> {
    let (program, mut diagnostics) = ModuleLoader::new()
        .with_source(file.as_ref(), source)
        .load_with_diagnostics(entry.as_ref())?;
    diagnostics.extend(TypeChecker::new().check_with_diagnostics(&program)?);
    Ok(diagnostics)
}

/// Checks a project with unsaved files encoded as a TOML `files` table.
///
/// Each key is a project-relative path and each value is its UTF-8 source text.
/// This editor protocol never writes buffers to disk.
pub fn diagnose_project_buffers(
    entry: impl AsRef<Path>,
    buffers: &str,
) -> Result<Vec<OrniDiagnostic>, OrniError> {
    diagnose_buffers(entry.as_ref(), buffers, false)
}

/// Checks a reusable module with unsaved buffers, without saving editor files.
pub fn diagnose_module_buffers(
    module: impl AsRef<Path>,
    buffers: &str,
) -> Result<Vec<OrniDiagnostic>, OrniError> {
    diagnose_buffers(module.as_ref(), buffers, true)
}

fn diagnose_buffers(
    entry: &Path,
    buffers: &str,
    as_module: bool,
) -> Result<Vec<OrniDiagnostic>, OrniError> {
    if buffers.len() > 8_388_608 {
        return Err(OrniError::module(
            1,
            1,
            "editor buffers exceed the limit of 8 MiB",
        ));
    }
    let document = buffers
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| OrniError::module(1, 1, format!("invalid editor buffers: {error}")))?;
    let files = document
        .get("files")
        .and_then(toml_edit::Item::as_table_like)
        .ok_or_else(|| OrniError::module(1, 1, "editor buffers require a files table"))?;
    if files.len() > 256 {
        return Err(OrniError::module(1, 1, "editor buffers exceed 256 files"));
    }
    let mut loader = ModuleLoader::new();
    for (file, value) in files.iter() {
        let source = value
            .as_str()
            .ok_or_else(|| OrniError::module(1, 1, "editor buffer values must be strings"))?;
        loader = loader.with_source(Path::new(file), source);
    }
    let (program, mut diagnostics) = if as_module {
        loader.load_library_with_diagnostics(entry)?
    } else {
        loader.load_with_diagnostics(entry)?
    };
    let checker = TypeChecker::new();
    diagnostics.extend(
        if as_module {
            checker.library()
        } else {
            checker
        }
        .check_with_diagnostics(&program)?,
    );
    Ok(diagnostics)
}

/// Checks a reusable Orni module and all modules imported by it.
pub fn check_module(path: impl AsRef<Path>) -> Result<(), OrniError> {
    let program = ModuleLoader::new().load_library(path.as_ref())?;
    TypeChecker::new().library().check(&program)
}

/// Checks a reusable module and returns every non-fatal compiler diagnostic.
pub fn diagnose_module(path: impl AsRef<Path>) -> Result<Vec<OrniDiagnostic>, OrniError> {
    let (program, mut diagnostics) =
        ModuleLoader::new().load_library_with_diagnostics(path.as_ref())?;
    diagnostics.extend(
        TypeChecker::new()
            .library()
            .check_with_diagnostics(&program)?,
    );
    Ok(diagnostics)
}

/// Runs an Orni program and returns everything it printed.
pub fn run(source: &str) -> Result<String, OrniError> {
    run_with_limits(source, ExecutionLimits::default())
}

/// Runs an Orni entry file together with its recursively imported modules.
pub fn run_file(path: impl AsRef<Path>) -> Result<String, OrniError> {
    let limits = Project::discover(path.as_ref())?
        .map_or_else(ExecutionLimits::default, |project| project.runtime);
    run_file_with_limits(path, limits)
}

/// Runs a file-based Orni program with explicit resource boundaries.
pub fn run_file_with_limits(
    path: impl AsRef<Path>,
    limits: ExecutionLimits,
) -> Result<String, OrniError> {
    let program = compile_file(path.as_ref())?;
    Interpreter::new(limits).execute(&program)
}

/// Runs an Orni test file with in-memory access to the application's HTTP routes.
#[cfg(not(target_arch = "wasm32"))]
pub fn run_api_test_file(
    entry: impl AsRef<Path>,
    test: impl AsRef<Path>,
    limits: ExecutionLimits,
) -> Result<String, OrniError> {
    let application = compile_file(entry.as_ref())?;
    let test_program = compile_file(test.as_ref())?;
    Interpreter::with_api_test(
        limits,
        crate::runtime::api_test_context(&application, limits),
    )
    .execute(&test_program)
}

/// Starts a native local HTTP server for routes declared in an Orni file.
#[cfg(not(target_arch = "wasm32"))]
pub fn serve_file(path: impl AsRef<Path>) -> Result<(), OrniError> {
    let (limits, http) = Project::discover(path.as_ref())?.map_or_else(
        || (ExecutionLimits::default(), crate::HttpLimits::default()),
        |project| (project.runtime, project.http),
    );
    let program = compile_file(path.as_ref())?;
    crate::runtime::serve(&program, limits, http)
}

/// Runs an Orni program with explicit resource boundaries.
pub fn run_with_limits(source: &str, limits: ExecutionLimits) -> Result<String, OrniError> {
    let program = compile(source)?;
    Interpreter::new(limits).execute(&program)
}

/// Produces a validated syntax tree shared by checking and execution.
pub(crate) fn compile(source: &str) -> Result<Vec<crate::syntax::Statement>, OrniError> {
    let tokens = Lexer::new(source).scan_tokens()?;
    let program = Parser::new(tokens).parse()?;
    TypeChecker::new().check(&program)?;
    Ok(program)
}

/// Resolves and validates a complete file-based module graph.
pub(crate) fn compile_file(path: &Path) -> Result<Vec<crate::syntax::Statement>, OrniError> {
    let program = ModuleLoader::new().load(path)?;
    TypeChecker::new().check(&program)?;
    Ok(program)
}
