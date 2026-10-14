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
mod runtime;
mod syntax;

pub use diagnostics::{DiagnosticSeverity, OrniDiagnostic, OrniError};

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
    let (program, diagnostics) = ModuleLoader::new().load_with_diagnostics(path.as_ref())?;
    TypeChecker::new().check(&program)?;
    Ok(diagnostics)
}

/// Checks a reusable Orni module and all modules imported by it.
///
/// Unlike [`check_file`], this rejects executable top-level statements such
/// as `print`, expressions, and conditionals. It is useful for validating a
/// library module independently from an application entry file.
pub fn check_module(path: impl AsRef<Path>) -> Result<(), OrniError> {
    let program = ModuleLoader::new().load_library(path.as_ref())?;
    TypeChecker::new().check(&program)
}

/// Checks a reusable module and returns every non-fatal compiler diagnostic.
pub fn diagnose_module(path: impl AsRef<Path>) -> Result<Vec<OrniDiagnostic>, OrniError> {
    let (program, diagnostics) =
        ModuleLoader::new().load_library_with_diagnostics(path.as_ref())?;
    TypeChecker::new().check(&program)?;
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
    run_file_with_limits(path, ExecutionLimits::default())
}

/// Runs a file-based Orni program with explicit resource boundaries.
pub fn run_file_with_limits(
    path: impl AsRef<Path>,
    limits: ExecutionLimits,
) -> Result<String, OrniError> {
    let program = compile_file(path.as_ref())?;
    Interpreter::new(limits).execute(&program)
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
        DiagnosticSeverity, ExecutionLimits, check, check_file, check_module, diagnose_file, run,
        run_file, run_with_limits,
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
    fn reports_unknown_variables_with_the_source_position() {
        let error = run("print missing;").expect_err("the program should fail");
        assert_eq!(
            error.to_string(),
            "Semantic error [ORNI-T001] at line 1, column 7 to line 1, column 14: undefined variable 'missing'"
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
            "Semantic error [ORNI-T001] at line 1, column 1 to line 1, column 7: return can only be used inside a function"
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
            "Semantic error [ORNI-T001] at line 1, column 5 to line 1, column 12: variable value expects int, but found string<3>"
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
            "imported modules may only contain imports, functions, and module variables"
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
}
