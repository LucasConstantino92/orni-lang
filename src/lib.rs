//! The Orni language runtime.
//!
//! This crate owns the complete source-to-execution pipeline. Source text is
//! tokenized by the lexer, transformed into an abstract syntax tree by the
//! parser, validated by the type checker, and finally evaluated by the
//! interpreter.

#![warn(missing_docs)]

use std::path::Path;

mod diagnostics;
mod frontend;
mod project;
#[cfg(test)]
mod regression_tests;
mod runtime;
mod syntax;

pub use diagnostics::{DiagnosticSeverity, OrniDiagnostic, OrniError};
pub use project::Project;

use frontend::{Lexer, ModuleLoader, Parser, TypeChecker};
pub use runtime::ExecutionLimits;
use runtime::Interpreter;

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
///
/// Unlike [`check_file`], this rejects executable top-level statements such
/// as `print`, expressions, and conditionals. It is useful for validating a
/// library module independently from an application entry file.
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
///
/// The returned string contains each value produced by an Orni `print`
/// statement, including its trailing newline. Lexical, syntax, and runtime
/// failures are returned as an [`OrniError`].
///
/// # Examples
///
/// ```
/// let output = orni::run("let name = \"Orni\"; print name;")?;
/// assert_eq!(output, "Orni\n");
/// # Ok::<(), orni::OrniError>(())
/// ```
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

/// Starts a native local HTTP server for the routes declared in an Orni file.
///
/// This development preview supports typed routes and in-memory CRUD stores.
/// The server listens on `127.0.0.1:3000` unless `app.port(...)` changes it in `main`.
#[cfg(not(target_arch = "wasm32"))]
pub fn serve_file(path: impl AsRef<Path>) -> Result<(), OrniError> {
    let limits = Project::discover(path.as_ref())?
        .map_or_else(ExecutionLimits::default, |project| project.runtime);
    let program = compile_file(path.as_ref())?;
    runtime::serve(&program, limits)
}

/// Runs an Orni program with explicit resource boundaries.
///
/// This is useful for servers that execute untrusted or user-provided source.
/// Values are still released automatically through Rust ownership when their
/// scope ends, while the supplied limits prevent unbounded resource growth.
pub fn run_with_limits(source: &str, limits: ExecutionLimits) -> Result<String, OrniError> {
    let program = compile(source)?;
    Interpreter::new(limits).execute(&program)
}

/// Produces a validated syntax tree shared by checking and execution.
fn compile(source: &str) -> Result<Vec<syntax::Statement>, OrniError> {
    let tokens = Lexer::new(source).scan_tokens()?;
    let program = Parser::new(tokens).parse()?;
    TypeChecker::new().check(&program)?;
    Ok(program)
}

/// Resolves and validates a complete file-based module graph.
fn compile_file(path: &Path) -> Result<Vec<syntax::Statement>, OrniError> {
    let program = ModuleLoader::new().load(path)?;
    TypeChecker::new().check(&program)?;
    Ok(program)
}

#[cfg(test)]
mod tests {
    use super::{
        DiagnosticSeverity, ExecutionLimits, Interpreter, check, check_file, check_module, compile,
        diagnose_file, run, run_file, run_with_limits,
    };

    #[test]
    fn executes_a_small_program() {
        let output = run(r#"
                // The first Orni program
                let language = "Orni";
                let version = 1;

                print language + " was born!";
                print version + 1;
            "#)
        .expect("the program should run");

        assert_eq!(output, "Orni was born!\n2\n");
    }

    #[test]
    fn validates_typed_data_models() {
        check(
            r#"
                model User {
                    id: uuid;
                    name: string<80>;
                    active: bool;
                }
            "#,
        )
        .expect("models should accept built-in field types");

        let error = check(
            r#"
                model User {
                    name: string;
                    name: string;
                }
            "#,
        )
        .expect_err("duplicate model fields should be rejected");
        assert!(
            error
                .to_string()
                .contains("model field name is already declared")
        );

        let error = check(
            r#"
                model User {
                    logger: Logger;
                }
            "#,
        )
        .expect_err("models cannot embed dependency contracts");
        assert!(
            error
                .to_string()
                .contains("model fields must use a built-in value type")
        );
    }

    #[test]
    fn validates_get_routes_and_handler_signatures() {
        check(
            r#"
            func hello() -> string { return "Hello, Orni!"; }
            route get "/hello" => hello;
        "#,
        )
        .expect("a zero-argument string handler should be valid");

        let error = check(
            r#"
            func hello() -> string { return "Hello, Orni!"; }
            route get "hello" => hello;
        "#,
        )
        .expect_err("route paths should be absolute");
        assert!(
            error
                .to_string()
                .contains("route paths must start with '/'")
        );

        let error = check(
            r#"
            func hello(name: string) -> string { return name; }
            route get "/hello" => hello;
        "#,
        )
        .expect_err("route handlers should match their path parameters");
        assert!(
            error
                .to_string()
                .contains("route declares 0 path parameter(s)")
        );

        check(
            r#"
            func hello(name: string) -> string { return "Hello, " + name + "!"; }
            route get "/hello/:name" => hello;
        "#,
        )
        .expect("a string path parameter should be supplied to its named handler parameter");

        check(
            r#"
            func user(id: uint16) -> string { return "User #" + id; }
            route get "/users/:id" => user;
        "#,
        )
        .expect("integer path parameters should be accepted");

        let error = check(
            r#"
            func user(session: Session) -> string { return "User"; }
            route get "/users/:session" => user;
        "#,
        )
        .expect_err("dependency contracts cannot be supplied by a URL segment");
        assert!(error.to_string().contains(
            "route path parameters must use string, integer, double, bool, char, or uuid types"
        ));

        let error = check(
            r#"
            func hello(id: string) -> string { return id; }
            route get "/hello/:name" => hello;
        "#,
        )
        .expect_err("handler parameter names should document their URL mapping");
        assert!(
            error
                .to_string()
                .contains("must match the route parameter ':name'")
        );
    }

    #[test]
    fn executes_a_route_handler_without_running_application_output() {
        let program = compile(
            r#"
                let greeting = "Hello";
                print "application startup output";
                func hello() -> string { return greeting + ", Orni!"; }
                route get "/hello" => hello;
            "#,
        )
        .expect("the HTTP example should compile");

        let response = Interpreter::new(ExecutionLimits::default())
            .execute_handler(&program, "hello", Vec::new())
            .expect("the route handler should return a body");

        assert_eq!(response.to_string(), "Hello, Orni!");
    }

    #[test]
    fn runs_main_after_top_level_statements() {
        let output = run(r#"
                let name = "Orni";
                print "Preparing";
                func main() -> unit { print "Hello, " + name + "!"; }
            "#)
        .expect("the application entry point should execute during a normal run");

        assert_eq!(output, "Preparing\nHello, Orni!\n");
    }

    #[test]
    fn requires_a_parameterless_unit_main_function() {
        let parameter_error = check("func main(name: string) -> unit { print name; }")
            .expect_err("main must not accept parameters");
        assert!(
            parameter_error
                .to_string()
                .contains("cannot declare parameters")
        );

        let return_error =
            check("func main() -> string { return \"no\"; }").expect_err("main must return unit");
        assert!(return_error.to_string().contains("must return unit"));
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn executes_main_once_before_starting_a_server() {
        let program = compile(
            r#"
                let application = "Orni API";
                func main() -> unit { print "Starting " + application; }
                func health() -> string { return "OK"; }
                route get "/health" => health;
            "#,
        )
        .expect("the application entry point should compile");

        let (output, _) = Interpreter::new(ExecutionLimits::default())
            .execute_main(&program)
            .expect("the application entry point should execute");

        assert_eq!(output, "Starting Orni API\n");
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn configures_the_development_server_from_main() {
        let program = compile(
            r#"
                func main() -> unit {
                    app.name("Orni API");
                    app.port(3200);
                }
            "#,
        )
        .expect("the application configuration should compile");

        let (_, application) = Interpreter::new(ExecutionLimits::default())
            .execute_main(&program)
            .expect("the application configuration should execute");

        assert_eq!(application.name.as_deref(), Some("Orni API"));
        assert_eq!(application.port, 3200);
    }

    #[test]
    fn evaluates_assertions_for_orni_tests() {
        run("assert 2 + 2 == 4;").expect("a true assertion should succeed");
        let error = run("assert false;").expect_err("a false assertion should fail");
        assert!(error.to_string().contains("assertion failed"));
    }

    #[test]
    fn reports_unknown_variables_with_the_source_position() {
        let error = run("print missing;").expect_err("the program should fail");
        assert_eq!(
            error.to_string(),
            "Semantic error [ORNI-E103] at line 1, column 7 to line 1, column 14: undefined variable 'missing'"
        );
    }

    #[test]
    fn executes_functions_with_isolated_scopes() {
        let output = run(r#"
                let name = "global";

                func greet(name: string) -> unit {
                    let message = "Hello, " + name;
                    print message;
                }

                greet("Orni");
                print name;
            "#)
        .expect("the program should run");

        assert_eq!(output, "Hello, Orni\nglobal\n");
    }

    #[test]
    fn reports_function_arity_errors() {
        let error = run("func greet(name: string) -> unit { print name; } greet();")
            .expect_err("the program should fail");

        assert!(
            error
                .to_string()
                .contains("function 'greet' expects 1 argument(s), but received 0")
        );
    }

    #[test]
    fn returns_values_and_stops_the_function_body() {
        let output = run(r#"
                func add(left: int, right: int) -> int {
                    return left + right;
                    print "unreachable";
                }

                print add(20, 22);
            "#)
        .expect("the program should run");

        assert_eq!(output, "42\n");
    }

    #[test]
    fn rejects_return_outside_a_function() {
        let error = run("return 42;").expect_err("the program should fail");

        assert_eq!(
            error.to_string(),
            "Semantic error [ORNI-E118] at line 1, column 1 to line 1, column 7: return can only be used inside a function"
        );
    }

    #[test]
    fn executes_conditionals_and_comparisons() {
        let output = run(r#"
                func classify(number: int) -> string {
                    if number >= 10 {
                        return "large";
                    } else {
                        return "small";
                    }
                }

                print classify(12);
                print 2 + 3 * 4 == 14;
                print !false;
            "#)
        .expect("the program should run");

        assert_eq!(output, "large\ntrue\ntrue\n");
    }

    #[test]
    fn isolates_variables_declared_inside_blocks() {
        let error = run(r#"
                if true {
                    let hidden = 42;
                    print hidden;
                }
                print hidden;
            "#)
        .expect_err("the program should fail");

        assert!(error.to_string().contains("undefined variable 'hidden'"));
    }

    #[test]
    fn uses_default_parameter_values() {
        let output = run(r#"
                func greet(name: string, title: string = "Developer") -> string {
                    return "Hello, " + title + " " + name;
                }

                print greet("Lucas");
                print greet("Lucas", "Architect");
            "#)
        .expect("the program should run");

        assert_eq!(output, "Hello, Developer Lucas\nHello, Architect Lucas\n");
    }

    #[test]
    fn rejects_arguments_with_the_wrong_type() {
        let error = run(r#"
                func greet(name: string) -> string {
                    return "Hello, " + name;
                }
                print greet(false);
            "#)
        .expect_err("the program should fail");

        assert!(
            error
                .to_string()
                .contains("function argument expects string, but found bool")
        );
    }

    #[test]
    fn accepts_explicit_and_inferred_variable_types() {
        let output = run(r#"
                let project: string = "Orni";
                let version = 1;
                let stable: bool = version >= 1;

                print project;
                print stable;
            "#)
        .expect("the program should run");

        assert_eq!(output, "Orni\ntrue\n");
    }

    #[test]
    fn rejects_a_variable_value_with_the_wrong_type() {
        let error = run("let version: int = \"one\";")
            .expect_err("the program should fail before execution");

        assert_eq!(
            error.to_string(),
            "Semantic error [ORNI-E107] at line 1, column 5 to line 1, column 12: variable value expects int, but found string<3>"
        );
    }

    #[test]
    fn distinguishes_int_double_bool_and_char_values() {
        let output = run(r#"
                let workers: int = 8;
                let ratio: double = 0.75;
                let enabled: bool = true;
                let initial: char = 'O';

                print workers + 2;
                print ratio * 2;
                print enabled;
                print initial;
            "#)
        .expect("the program should run");

        assert_eq!(output, "10\n1.5\ntrue\nO\n");
    }

    #[test]
    fn validates_and_preserves_uuid_values() {
        let output = run(r#"
                let user_id: uuid = "550e8400-e29b-41d4-a716-446655440000";
                print user_id;
            "#)
        .expect("a valid UUID string should initialize a uuid value");
        assert_eq!(output, "550e8400-e29b-41d4-a716-446655440000\n");

        let error = check(r#"let user_id: uuid = "not-a-uuid";"#)
            .expect_err("invalid UUID literals should be diagnosed before execution");
        assert!(error.to_string().contains("value is not a valid UUID"));
    }

    #[test]
    fn promotes_int_to_double_without_narrowing_double_to_int() {
        let output = run("let measurement: double = 2; print measurement + 0.5;")
            .expect("int values should widen to double declarations");
        assert_eq!(output, "2.5\n");

        let error = run("let count: int = 2.5;")
            .expect_err("double values must not narrow to int implicitly");
        assert!(
            error
                .to_string()
                .contains("variable value expects int, but found double")
        );
    }

    #[test]
    fn reports_checked_integer_overflow() {
        let error = run("let maximum: int8 = 127; let one: int8 = 1; print maximum + one;")
            .expect_err("integer arithmetic must not overflow silently");

        assert!(error.to_string().contains("out of range for int8"));
    }

    #[test]
    fn infers_the_smallest_safe_signed_integer_type() {
        let output = run(r#"
                let retries = 3;
                let port = 8080;
                let users = 250000;
                let distance = 9000000000;

                print retries;
                print port;
                print users;
                print distance;
            "#)
        .expect("inferred integer widths should execute normally");

        assert_eq!(output, "3\n8080\n250000\n9000000000\n");

        let error = run("let maximum = 127; let one = 1; print maximum + one;")
            .expect_err("inferred int8 arithmetic must remain checked");
        assert!(error.to_string().contains("out of range for int8"));
    }

    #[test]
    fn supports_fixed_width_signed_and_unsigned_integers() {
        let output = run(r#"
                let signed: int8 = -128;
                let port: uint16 = 8080;
                let users: int32 = 250000;
                let maximum: uint64 = 18446744073709551615;

                print signed;
                print port;
                print users;
                print maximum;
            "#)
        .expect("fixed-width integers should accept values in range");

        assert_eq!(output, "-128\n8080\n250000\n18446744073709551615\n");
    }

    #[test]
    fn rejects_integer_conversions_outside_the_target_range() {
        let error = run("let retries: uint8 = 256;")
            .expect_err("checked integer conversion must reject values outside its range");

        assert!(
            error
                .to_string()
                .contains("value 256 is out of range for uint8")
        );
    }

    #[test]
    fn infers_and_enforces_utf8_string_capacities() {
        let output = run(r#"
                let name = "Lucas";
                let prefix: string = "Hi";
                let compact: string<3> = prefix + "!";
                let greeting: string<12> = "Hello, " + name;
                let accented: string<4> = "Olá";

                print compact;
                print greeting;
                print accented;
            "#)
        .expect("inferred string capacities should fit their declarations");

        assert_eq!(output, "Hi!\nHello, Lucas\nOlá\n");

        let error = check("let name: string<4> = \"Lucas\";")
            .expect_err("a literal that exceeds its capacity must fail during checking");
        assert!(
            error
                .to_string()
                .contains("expects string<4>, but found string<5>")
        );
    }

    #[test]
    fn infers_string_capacities_across_function_calls() {
        let output = run(r#"
                func greet(name: string, greeting: string = "Hello") -> string {
                    return greeting + ", " + name;
                }

                let default_greeting: string<11> = greet("Orni");
                let custom_greeting: string<13> = greet("Orni", "Welcome");

                print default_greeting;
                print custom_greeting;
            "#)
        .expect("bare string signatures should infer capacities at each call site");

        assert_eq!(output, "Hello, Orni\nWelcome, Orni\n");
    }

    #[test]
    fn enforces_configurable_runtime_resource_limits() {
        let string_error = run_with_limits(
            "let value = \"abcdef\"; print value;",
            ExecutionLimits {
                max_string_bytes: 5,
                ..ExecutionLimits::default()
            },
        )
        .expect_err("a retained string must respect the per-value limit");
        assert!(string_error.to_string().contains("runtime limit is 5"));

        let retained_error = run_with_limits(
            "let first = \"abc\"; let second = \"def\"; print second;",
            ExecutionLimits {
                max_retained_string_bytes: 5,
                ..ExecutionLimits::default()
            },
        )
        .expect_err("active scopes must respect their retained string limit");
        assert!(
            retained_error
                .to_string()
                .contains("retained strings exceed")
        );

        let recursion_error = run_with_limits(
            "func recurse(depth: int) -> int { return recurse(depth + 1); } print recurse(0);",
            ExecutionLimits {
                max_call_depth: 3,
                ..ExecutionLimits::default()
            },
        )
        .expect_err("recursive calls must stop at the configured depth");
        assert!(recursion_error.to_string().contains("call depth exceeds"));

        let operation_error = run_with_limits(
            "let value = 1; print value;",
            ExecutionLimits {
                max_operations: 2,
                ..ExecutionLimits::default()
            },
        )
        .expect_err("execution must stop when its operation budget is exhausted");
        assert!(
            operation_error
                .to_string()
                .contains("limit of 2 operation(s)")
        );
    }

    #[test]
    fn loads_public_functions_from_file_modules() {
        let output = run_file("examples/05_modules.orni")
            .expect("the entry file should load its public module functions");

        assert_eq!(output, "Hello, Orni\nWelcome to the, Framework\nOrni\n");

        let default_namespace = run_file("test/modules/default_entry.orni")
            .expect("the final path segment should become the default namespace");
        assert_eq!(default_namespace, "hidden\n");
    }

    #[test]
    fn rejects_circular_and_private_module_imports() {
        let circular = check_file("test/modules/circular_a.orni")
            .expect_err("a circular module graph must be rejected");
        assert!(circular.to_string().contains("circular import detected"));

        let private = check_file("test/modules/private_entry.orni")
            .expect_err("underscore-prefixed members must remain private");
        assert!(
            private
                .to_string()
                .contains("private member '_hidden' cannot be accessed")
        );

        let obsolete_public = check("pub func greet() -> string { return \"Hello\"; }")
            .expect_err("the pub keyword should be replaced by underscore privacy");
        assert!(obsolete_public.to_string().contains("'pub' is unnecessary"));
    }

    #[test]
    fn rejects_ambiguous_module_names() {
        let duplicate = check_file("test/modules/duplicate_member.orni")
            .expect_err("a module member must have exactly one declaration");
        assert!(
            duplicate
                .to_string()
                .contains("module member 'status' is already declared")
        );

        let alias = check_file("test/modules/alias_collision.orni")
            .expect_err("an import alias must not shadow a module declaration");
        assert!(
            alias
                .to_string()
                .contains("module namespace 'service' conflicts with a declaration")
        );
    }

    #[test]
    fn protects_private_members_in_nested_modules() {
        let error = check_file("test/modules/nested_private_entry.orni")
            .expect_err("nested private members must not escape their declaring module");
        let diagnostic = error.to_string();

        assert!(diagnostic.contains("private member 'inner._hidden' cannot be accessed"));
        assert!(diagnostic.contains("nested_private_entry.orni' at line 3, column 7"));
    }

    #[test]
    fn reports_the_exact_module_that_contains_a_semantic_error() {
        let error = check_file("test/modules/semantic_error_entry.orni")
            .expect_err("module diagnostics must preserve their source file");
        let diagnostic = error.to_string();

        assert!(diagnostic.contains("semantic_error.orni' at line 2"));
        assert!(diagnostic.contains("undefined variable 'missing'"));
    }

    #[test]
    fn checks_a_reusable_module_without_an_entry_program() {
        check_module("examples/modules/greeting.orni")
            .expect("a declaration-only module should pass isolated validation");

        let error = check_module("examples/05_modules.orni")
            .expect_err("an entry program must not be accepted as a reusable module");
        assert!(error.to_string().contains(
            "imported modules may only contain imports, contracts, providers, functions, and module variables"
        ));
    }

    #[test]
    fn collects_multiple_independent_semantic_errors() {
        let error = check("print first_missing;\nprint second_missing;")
            .expect_err("both invalid statements should be diagnosed");

        assert_eq!(error.diagnostics().len(), 2);
        assert!(
            error
                .to_string()
                .contains("undefined variable 'first_missing'")
        );
        assert!(
            error
                .to_string()
                .contains("undefined variable 'second_missing'")
        );
    }

    #[test]
    fn reports_structured_unused_import_warnings() {
        let diagnostics = diagnose_file("test/modules/unused_import_entry.orni")
            .expect("warnings must not fail compilation");
        let [warning] = diagnostics.as_slice() else {
            panic!("expected exactly one warning");
        };

        assert_eq!(warning.code(), "ORNI-W001");
        assert_eq!(warning.severity(), DiagnosticSeverity::Warning);
        assert_eq!(
            warning.source(),
            Some("test/modules/unused_import_entry.orni")
        );
        assert_eq!((warning.line(), warning.column()), (1, 8));
        assert_eq!((warning.end_line(), warning.end_column()), (1, 15));
        assert!(warning.message().contains("imported but never used"));
    }

    #[test]
    fn imports_selected_public_module_members_into_the_local_scope() {
        let output = run_file("examples/06_selective_imports.orni")
            .expect("selected public members should be directly callable");

        assert_eq!(output, "Hello, Orni\nOrni\n");
    }

    #[test]
    fn validates_selective_import_boundaries() {
        let private = check_file("test/modules/selective_private.orni")
            .expect_err("private members must not be selectively imported");
        assert!(private.to_string().contains("private member '_hidden'"));

        let unknown = check_file("test/modules/selective_unknown.orni")
            .expect_err("unknown selected members must be rejected");
        assert!(
            unknown
                .to_string()
                .contains("no public member named 'missing'")
        );

        let collision = check_file("test/modules/selective_collision.orni")
            .expect_err("selected members must not shadow local declarations");
        assert!(
            collision
                .to_string()
                .contains("selectively imported member 'visible' conflicts")
        );
    }

    #[test]
    fn reports_unused_selective_imports() {
        let diagnostics = diagnose_file("test/modules/selective_unused.orni")
            .expect("an unused selected member is a non-fatal warning");
        let [warning] = diagnostics.as_slice() else {
            panic!("expected exactly one warning");
        };

        assert_eq!(warning.code(), "ORNI-W002");
        assert!(warning.message().contains("member 'visible' is never used"));
    }

    #[test]
    fn suggests_the_closest_module_name() {
        let error = check_file("test/modules/missing_suggestion.orni")
            .expect_err("a misspelled import must fail with a useful suggestion");

        assert!(error.to_string().contains("did you mean 'private'?"));
    }

    #[test]
    fn reports_unused_private_functions() {
        let diagnostics = diagnose_file("test/modules/unused_private_entry.orni")
            .expect("an unused private function is a non-fatal warning");
        let warning = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code() == "ORNI-W003")
            .expect("the private function warning should be present");

        assert!(
            warning
                .message()
                .contains("private function '_unused' is never used")
        );
    }

    #[test]
    fn injects_a_bound_provider_through_its_contract() {
        let output = run(r#"
                contract Logger {
                    func log(message: string) -> unit;
                }
                provider ConsoleLogger for Logger {
                    func log(message: string) -> unit { print message; }
                }
                bind Logger to ConsoleLogger;
                func start(logger: Logger) -> unit {
                    logger.log("Orni application started");
                }
                start();
            "#)
        .expect("the bound provider should be injected automatically");

        assert_eq!(output, "Orni application started\n");
    }

    #[test]
    fn validates_provider_contract_implementations() {
        let missing_method = check(
            r#"
                contract Logger { func log(message: string) -> unit; }
                provider EmptyLogger for Logger {}
            "#,
        )
        .expect_err("a provider must implement every contract method");
        assert!(
            missing_method
                .to_string()
                .contains("provider 'EmptyLogger' is missing contract method 'log'")
        );

        let wrong_signature = check(
            r#"
                contract Logger { func log(message: string) -> unit; }
                provider NumberLogger for Logger {
                    func log(message: int) -> unit { print message; }
                }
            "#,
        )
        .expect_err("provider signatures must match their contracts");
        assert!(
            wrong_signature
                .to_string()
                .contains("does not match contract 'Logger'")
        );
    }

    #[test]
    fn requires_a_binding_before_dependency_injection() {
        let error = check(
            r#"
                contract Logger { func log(message: string) -> unit; }
                provider ConsoleLogger for Logger {
                    func log(message: string) -> unit { print message; }
                }
                func start(logger: Logger) -> unit { logger.log("start"); }
                start();
            "#,
        )
        .expect_err("injected contracts need one provider binding");

        assert!(
            error
                .to_string()
                .contains("contract 'Logger' has no provider binding")
        );
    }

    #[test]
    fn injects_contract_dependencies_into_provider_methods() {
        let output = run(r#"
                contract Logger { func log(message: string) -> unit; }
                contract Audit { func record(message: string) -> unit; }
                provider ConsoleLogger for Logger {
                    func log(message: string) -> unit { print message; }
                }
                provider ConsoleAudit(logger: Logger) for Audit {
                    func record(message: string) -> unit {
                        logger.log("AUDIT: " + message);
                    }
                }
                bind Logger to ConsoleLogger;
                bind Audit to ConsoleAudit;
                func start(audit: Audit) -> unit { audit.record("ready"); }
                start();
            "#)
        .expect("provider dependencies should be injected automatically");

        assert_eq!(output, "AUDIT: ready\n");
    }

    #[test]
    fn validates_provider_dependencies_before_execution() {
        let missing_binding = check(
            r#"
                contract Logger { func log(message: string) -> unit; }
                contract Audit { func record(message: string) -> unit; }
                provider ConsoleAudit(logger: Logger) for Audit {
                    func record(message: string) -> unit { logger.log(message); }
                }
                bind Audit to ConsoleAudit;
            "#,
        )
        .expect_err("provider dependencies require a configured binding");
        assert!(
            missing_binding
                .to_string()
                .contains("Audit -> Logger; contract 'Logger' has no provider binding")
        );

        let invalid_type = check(
            r#"
                contract Audit { func record(message: string) -> unit; }
                provider ConsoleAudit(prefix: string) for Audit {
                    func record(message: string) -> unit { print message; }
                }
                bind Audit to ConsoleAudit;
            "#,
        )
        .expect_err("provider dependencies must be contracts");
        assert!(
            invalid_type
                .to_string()
                .contains("provider dependency 'prefix' must use a contract type")
        );
    }

    #[test]
    fn reports_the_complete_missing_provider_dependency_chain() {
        let error = check(
            r#"
                contract Clock { func now() -> int; }
                contract Logger { func log(message: string) -> unit; }
                contract Audit { func record(message: string) -> unit; }

                provider ConsoleAudit(logger: Logger) for Audit {
                    func record(message: string) -> unit { logger.log(message); }
                }
                provider TimestampLogger(clock: Clock) for Logger {
                    func log(message: string) -> unit { print message; }
                }

                bind Audit to ConsoleAudit;
                bind Logger to TimestampLogger;
            "#,
        )
        .expect_err("a missing transitive binding must report its complete chain");

        assert!(error.to_string().contains(
            "provider dependency chain is incomplete: Audit -> Logger -> Clock; contract 'Clock' has no provider binding"
        ));
    }

    #[test]
    fn rejects_circular_provider_dependency_graphs() {
        let error = check(
            r#"
                contract Logger { func log(message: string) -> unit; }
                contract Audit { func record(message: string) -> unit; }

                provider ConsoleLogger(audit: Audit) for Logger {
                    func log(message: string) -> unit { audit.record(message); }
                }
                provider ConsoleAudit(logger: Logger) for Audit {
                    func record(message: string) -> unit { logger.log(message); }
                }

                bind Logger to ConsoleLogger;
                bind Audit to ConsoleAudit;
            "#,
        )
        .expect_err("provider cycles must fail before execution");

        assert!(
            error
                .to_string()
                .contains("circular provider dependency detected: Audit -> Logger -> Audit")
        );
    }

    #[test]
    fn warns_about_bindings_that_are_never_injected() {
        // Keep this assertion independent from examples edited during manual testing.
        let fixture = "examples/experimental/errors/unused_binding.orni";
        let diagnostics = super::diagnose_file_with_source(fixture, fixture, r#"
            contract Logger { func log(message: string) -> unit; }
            provider ConsoleLogger for Logger { func log(message: string) -> unit { print message; } }
            bind Logger to ConsoleLogger;
        "#).expect("an unused binding should remain a valid program");
        let warning = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code() == "ORNI-W004")
            .expect("the unused binding warning should be present");

        assert_eq!(warning.severity(), DiagnosticSeverity::Warning);
        assert!(
            warning
                .message()
                .contains("binding for contract 'Logger' is never injected")
        );
    }

    #[test]
    fn resolves_framework_declarations_independently_of_source_order() {
        let output = run(r#"
                start();
                bind Audit to ConsoleAudit;
                bind Logger to ConsoleLogger;

                provider ConsoleAudit(logger: Logger) for Audit {
                    func record(message: string) -> unit { logger.log(message); }
                }
                contract Audit { func record(message: string) -> unit; }

                func start(audit: Audit) -> unit { audit.record("ordered"); }

                provider ConsoleLogger for Logger {
                    func log(message: string) -> unit { print message; }
                }
                contract Logger { func log(message: string) -> unit; }
            "#)
        .expect("framework declarations should be resolved before execution");

        assert_eq!(output, "ordered\n");
    }

    #[test]
    fn loads_contracts_and_providers_from_separate_modules() {
        let output = run_file("examples/experimental/framework_project/main.orni")
            .expect("framework declarations should preserve one identity across modules");

        assert_eq!(output, "AUDIT: Orni modular application started\n");
    }
}
