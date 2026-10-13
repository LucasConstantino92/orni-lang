use crate::token::Token;

/// A built-in type understood by Orni's semantic analyzer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TypeName {
    /// A signed 64-bit integer.
    Int,
    /// A double-precision floating-point number.
    Double,
    /// A UTF-8 string.
    String,
    /// A boolean value.
    Bool,
    /// A single Unicode scalar value.
    Char,
    /// The absence of a meaningful value.
    Unit,
}

impl std::fmt::Display for TypeName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Int => write!(formatter, "int"),
            Self::Double => write!(formatter, "double"),
            Self::String => write!(formatter, "string"),
            Self::Bool => write!(formatter, "bool"),
            Self::Char => write!(formatter, "char"),
            Self::Unit => write!(formatter, "unit"),
        }
    }
}

/// A named function parameter with an explicit type and optional default.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Parameter {
    /// The parameter name and its source position.
    pub name: Token,
    /// The type accepted by the parameter.
    pub type_name: TypeName,
    /// The expression used when the caller omits this argument.
    pub default_value: Option<Expression>,
}

/// A complete action that can appear in an Orni program.
///
/// Statements perform work but do not directly produce a reusable value.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Statement {
    /// Declares a variable in the active scope.
    Let {
        /// The variable name and its source position.
        name: Token,
        /// An optional type required by the declaration.
        declared_type: Option<TypeName>,
        /// The expression evaluated before the variable is defined.
        value: Expression,
    },
    /// Declares a named function.
    Function {
        /// The function name and its source position.
        name: Token,
        /// The ordered names bound to arguments during a call.
        parameters: Vec<Parameter>,
        /// The value type produced by the function.
        return_type: TypeName,
        /// The statements executed when the function is called.
        body: Vec<Statement>,
    },
    /// Evaluates and writes a value to the program output.
    Print(Expression),
    /// Stops the current function and optionally produces a value.
    Return {
        /// The `return` token used when reporting an invalid top-level return.
        keyword: Token,
        /// The value returned by the function, or `None` for `return;`.
        value: Option<Expression>,
    },
    /// Executes one of two branches according to a boolean condition.
    If {
        /// The `if` token used when reporting an invalid condition.
        keyword: Token,
        /// The boolean expression that selects a branch.
        condition: Expression,
        /// The branch executed when the condition is `true`.
        then_branch: Box<Statement>,
        /// The optional branch executed when the condition is `false`.
        else_branch: Option<Box<Statement>>,
    },
    /// Executes statements inside a nested lexical scope.
    Block(Vec<Statement>),
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
    /// A signed 64-bit integer.
    Int(i64),
    /// A double-precision floating-point number.
    Double(f64),
    /// A UTF-8 string.
    String(String),
    /// A boolean value.
    Bool(bool),
    /// A single Unicode scalar value.
    Char(char),
    /// The absence of a meaningful value, currently returned by functions.
    Unit,
}

impl std::fmt::Display for Value {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Int(number) => write!(formatter, "{number}"),
            Self::Double(number) => write!(formatter, "{number}"),
            Self::String(value) => write!(formatter, "{value}"),
            Self::Bool(value) => write!(formatter, "{value}"),
            Self::Char(value) => write!(formatter, "{value}"),
            Self::Unit => write!(formatter, "unit"),
        }
    }
}

impl Value {
    /// Returns the built-in Orni type represented by this runtime value.
    pub fn type_name(&self) -> TypeName {
        match self {
            Self::Int(_) => TypeName::Int,
            Self::Double(_) => TypeName::Double,
            Self::String(_) => TypeName::String,
            Self::Bool(_) => TypeName::Bool,
            Self::Char(_) => TypeName::Char,
            Self::Unit => TypeName::Unit,
        }
    }
}
