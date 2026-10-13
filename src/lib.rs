//! The Orni language runtime.
//!
//! This crate owns the complete source-to-execution pipeline. Source text is
//! tokenized by the lexer, transformed into an abstract syntax tree by the
//! parser, validated by the type checker, and finally evaluated by the
//! interpreter.

#![warn(missing_docs)]

mod diagnostics;
mod frontend;
mod runtime;
mod syntax;

pub use diagnostics::OrniError;

use frontend::{Lexer, Parser, TypeChecker};
use runtime::Interpreter;

/// Checks an Orni program without executing it.
///
/// This performs lexical, syntax, and semantic validation. Editors can use
/// it to report diagnostics without triggering program behavior.
pub fn check(source: &str) -> Result<(), OrniError> {
    compile(source).map(|_| ())
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
    let program = compile(source)?;
    Interpreter::new().execute(&program)
}

/// Produces a validated syntax tree shared by checking and execution.
fn compile(source: &str) -> Result<Vec<syntax::Statement>, OrniError> {
    let tokens = Lexer::new(source).scan_tokens()?;
    let program = Parser::new(tokens).parse()?;
    TypeChecker::new().check(&program)?;
    Ok(program)
}

#[cfg(test)]
mod tests {
    use super::{check, run};

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
            "Semantic error at line 1, column 7: undefined variable 'missing'"
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
            "Semantic error at line 1, column 1: return can only be used inside a function"
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
            "Semantic error at line 1, column 5: variable value expects int, but found string<3>"
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
                let greeting: string<12> = "Hello, " + name;
                let accented: string<4> = "Olá";

                print greeting;
                print accented;
            "#)
        .expect("inferred string capacities should fit their declarations");

        assert_eq!(output, "Hello, Lucas\nOlá\n");

        let error = check("let name: string<4> = \"Lucas\";")
            .expect_err("a literal that exceeds its capacity must fail during checking");
        assert!(
            error
                .to_string()
                .contains("expects string<4>, but found string<5>")
        );
    }
}
