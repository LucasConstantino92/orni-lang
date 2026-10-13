//! The Orni language runtime.
//!
//! This crate owns the complete source-to-execution pipeline. Source text is
//! tokenized by the lexer, transformed into an abstract syntax tree by the
//! parser, and finally evaluated by the interpreter.

#![warn(missing_docs)]

mod ast;
mod environment;
mod error;
mod interpreter;
mod lexer;
mod parser;
mod token;

pub use error::OrniError;

use interpreter::Interpreter;
use lexer::Lexer;
use parser::Parser;

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
    let tokens = Lexer::new(source).scan_tokens()?;
    let program = Parser::new(tokens).parse()?;
    Interpreter::new().execute(&program)
}

#[cfg(test)]
mod tests {
    use super::run;

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
            "Runtime error at line 1, column 7: undefined variable 'missing'"
        );
    }

    #[test]
    fn executes_functions_with_isolated_scopes() {
        let output = run(r#"
                let name = "global";

                fn greet(name) {
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
        let error =
            run("fn greet(name) { print name; } greet();").expect_err("the program should fail");

        assert_eq!(
            error.to_string(),
            "Runtime error at line 1, column 38: function 'greet' expects 1 argument(s), but received 0"
        );
    }

    #[test]
    fn returns_values_and_stops_the_function_body() {
        let output = run(r#"
                fn add(left, right) {
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
            "Runtime error at line 1, column 1: return can only be used inside a function"
        );
    }

    #[test]
    fn executes_conditionals_and_comparisons() {
        let output = run(r#"
                fn classify(number) {
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
}
