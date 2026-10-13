use crate::token::Token;

/// A complete action that can appear in an Orni program.
///
/// Statements perform work but do not directly produce a reusable value.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Statement {
    /// Declares a variable in the active scope.
    Let {
        /// The variable name and its source position.
        name: Token,
        /// The expression evaluated before the variable is defined.
        value: Expression,
    },
    /// Declares a named function.
    Function {
        /// The function name and its source position.
        name: Token,
        /// The ordered names bound to arguments during a call.
        parameters: Vec<Token>,
        /// The statements executed when the function is called.
        body: Vec<Statement>,
    },
    /// Evaluates and writes a value to the program output.
    Print(Expression),
    /// Evaluates an expression and discards its resulting value.
    Expression(Expression),
}

/// A value-producing node in Orni's abstract syntax tree.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Expression {
    /// A value written directly in source code.
    Literal(Value),
    /// A variable lookup.
    Variable(Token),
    /// An operator applied to one operand.
    Unary {
        /// The operator token and its source position.
        operator: Token,
        /// The operand evaluated before applying the operator.
        right: Box<Expression>,
    },
    /// An operator applied to two operands.
    Binary {
        /// The left-hand operand.
        left: Box<Expression>,
        /// The operator token and its source position.
        operator: Token,
        /// The right-hand operand.
        right: Box<Expression>,
    },
    /// A function invocation.
    Call {
        /// The expression expected to resolve to a function name.
        callee: Box<Expression>,
        /// The closing parenthesis used when reporting call-site errors.
        closing_parenthesis: Token,
        /// The argument expressions in source order.
        arguments: Vec<Expression>,
    },
    /// An expression whose precedence was explicitly grouped with parentheses.
    Grouping(Box<Expression>),
}

/// A runtime value understood by the current Orni interpreter.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    /// A double-precision number.
    Number(f64),
    /// A UTF-8 string.
    String(String),
    /// A boolean value.
    Boolean(bool),
    /// The absence of a meaningful value, currently returned by functions.
    Unit,
}

impl std::fmt::Display for Value {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Number(number) if number.fract() == 0.0 => write!(formatter, "{number:.0}"),
            Self::Number(number) => write!(formatter, "{number}"),
            Self::String(value) => write!(formatter, "{value}"),
            Self::Boolean(value) => write!(formatter, "{value}"),
            Self::Unit => write!(formatter, "unit"),
        }
    }
}
