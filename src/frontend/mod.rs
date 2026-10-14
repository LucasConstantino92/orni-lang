//! Source analysis stages that produce and validate Orni syntax.

mod lexer;
mod modules;
mod parser;
mod type_checker;

pub(crate) use lexer::Lexer;
pub(crate) use modules::ModuleLoader;
pub(crate) use parser::Parser;
pub(crate) use type_checker::TypeChecker;
