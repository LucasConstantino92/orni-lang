use std::sync::Arc;

use uuid::Uuid;

use super::Token;

/// The storage range selected for an integer value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IntegerType {
    /// An 8-bit signed integer.
    Int8,
    /// A 16-bit signed integer.
    Int16,
    /// A 32-bit signed integer.
    Int32,
    /// A 64-bit signed integer, also spelled `int`.
    Int64,
    /// An 8-bit unsigned integer.
    UInt8,
    /// A 16-bit unsigned integer.
    UInt16,
    /// A 32-bit unsigned integer.
    UInt32,
    /// A 64-bit unsigned integer.
    UInt64,
}

impl IntegerType {
    /// Chooses the smallest signed integer type that contains a literal.
    pub fn smallest_signed(value: i128) -> Self {
        if Self::Int8.contains(value) {
            Self::Int8
        } else if Self::Int16.contains(value) {
            Self::Int16
        } else if Self::Int32.contains(value) {
            Self::Int32
        } else {
            Self::Int64
        }
    }

    /// Returns whether a value fits within this integer type.
    pub fn contains(self, value: i128) -> bool {
        match self {
            Self::Int8 => (i8::MIN as i128..=i8::MAX as i128).contains(&value),
            Self::Int16 => (i16::MIN as i128..=i16::MAX as i128).contains(&value),
            Self::Int32 => (i32::MIN as i128..=i32::MAX as i128).contains(&value),
            Self::Int64 => (i64::MIN as i128..=i64::MAX as i128).contains(&value),
            Self::UInt8 => (0..=u8::MAX as i128).contains(&value),
            Self::UInt16 => (0..=u16::MAX as i128).contains(&value),
            Self::UInt32 => (0..=u32::MAX as i128).contains(&value),
            Self::UInt64 => (0..=u64::MAX as i128).contains(&value),
        }
    }
}

impl std::fmt::Display for IntegerType {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Int8 => write!(formatter, "int8"),
            Self::Int16 => write!(formatter, "int16"),
            Self::Int32 => write!(formatter, "int32"),
            Self::Int64 => write!(formatter, "int"),
            Self::UInt8 => write!(formatter, "uint8"),
            Self::UInt16 => write!(formatter, "uint16"),
            Self::UInt32 => write!(formatter, "uint32"),
            Self::UInt64 => write!(formatter, "uint64"),
        }
    }
}

/// A built-in type understood by Orni's semantic analyzer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TypeName {
    /// An integer with an explicit storage range.
    Integer(IntegerType),
    /// A double-precision floating-point number.
    Double,
    /// A UTF-8 string with an optional maximum byte capacity.
    String(Option<usize>),
    /// A boolean value.
    Bool,
    /// A single Unicode scalar value.
    Char,
    /// A universally unique identifier stored as 16 bytes.
    Uuid,
    /// The absence of a meaningful value.
    Unit,
    /// A user-defined dependency contract.
    Contract(String),
    /// A user-defined typed data model.
    Model(String),
    /// An explicit HTTP response produced by a route handler.
    Response,
}

impl std::fmt::Display for TypeName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Integer(integer_type) => write!(formatter, "{integer_type}"),
            Self::Double => write!(formatter, "double"),
            Self::String(Some(max_bytes)) => write!(formatter, "string<{max_bytes}>"),
            Self::String(None) => write!(formatter, "string"),
            Self::Bool => write!(formatter, "bool"),
            Self::Char => write!(formatter, "char"),
            Self::Uuid => write!(formatter, "uuid"),
            Self::Unit => write!(formatter, "unit"),
            Self::Contract(name) => write!(formatter, "{name}"),
            Self::Model(name) => write!(formatter, "{name}"),
            Self::Response => write!(formatter, "response"),
        }
    }
}

/// A function requirement declared by a dependency contract.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ContractMethod {
    /// The required method name.
    pub name: Token,
    /// The ordered method parameters.
    pub parameters: Vec<Parameter>,
    /// The value produced by the method.
    pub return_type: TypeName,
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

/// One named, typed field in a data model.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ModelField {
    /// The field name and its source position.
    pub name: Token,
    /// The value type accepted by this field.
    pub type_name: TypeName,
}

/// A complete action that can appear in an Orni program.
///
/// Statements perform work but do not directly produce a reusable value.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Statement {
    /// Imports public declarations from another Orni source module.
    Import {
        /// The `import` token used for module diagnostics.
        keyword: Token,
        /// Identifier segments resolved as a relative `.orni` path.
        path: Vec<Token>,
        /// Optional namespace chosen by `as`.
        alias: Option<Token>,
        /// Optional public members imported directly into the local scope.
        members: Option<Vec<Token>>,
    },
    /// Declares the callable surface required from a provider.
    Contract {
        /// The contract name.
        name: Token,
        /// Required method signatures.
        methods: Vec<ContractMethod>,
    },
    /// Declares the typed shape of application data.
    Model {
        /// The model name.
        name: Token,
        /// Fields required by values of this model.
        fields: Vec<ModelField>,
    },
    /// Declares an in-memory collection of values for one model.
    Store {
        /// The `store` token used for diagnostics.
        keyword: Token,
        /// The collection name used by route handlers.
        name: Token,
        /// The model retained by this collection.
        model: Token,
    },
    /// Implements one dependency contract.
    Provider {
        /// The provider name.
        name: Token,
        /// Contracts injected into every provider method.
        dependencies: Vec<Parameter>,
        /// The implemented contract name.
        contract: Token,
        /// Provider method implementations.
        methods: Vec<Statement>,
    },
    /// Selects the provider injected for a contract.
    Bind {
        /// The `bind` keyword used for diagnostics.
        keyword: Token,
        /// The contract being configured.
        contract: Token,
        /// The provider selected for the contract.
        provider: Token,
    },
    /// Maps an HTTP GET path to a zero-argument string handler.
    Route {
        /// The `route` token used for diagnostics.
        keyword: Token,
        /// The HTTP method accepted by the route.
        method: Token,
        /// The URL path accepted by the route.
        path: Token,
        /// The function called to produce the response body.
        handler: Token,
    },
    /// Declares a variable in the active scope.
    Let {
        /// The variable name and its source position.
        name: Token,
        /// An optional type required by the declaration.
        declared_type: Option<TypeName>,
        /// The expression evaluated before the variable is defined.
        value: Expression,
    },
    /// Verifies that an expression evaluates to true.
    Assert {
        /// The keyword used for diagnostics.
        keyword: Token,
        /// The boolean condition required by the assertion.
        expression: Expression,
    },
    /// Groups assertions into one named test scenario.
    Test {
        /// The test keyword used in diagnostics.
        keyword: Token,
        /// Human-readable scenario name.
        name: Token,
        /// Statements evaluated by this scenario.
        body: Vec<Statement>,
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
    Print {
        /// The `print` token used when reporting output limit errors.
        keyword: Token,
        /// The value written to the captured program output.
        expression: Expression,
    },
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
    /// An integer tagged with its selected storage range.
    Integer {
        /// The mathematical integer value.
        value: i128,
        /// The range enforced by arithmetic and conversions.
        integer_type: IntegerType,
    },
    /// A double-precision floating-point number.
    Double(f64),
    /// A UTF-8 string.
    String(Arc<str>),
    /// A boolean value.
    Bool(bool),
    /// A single Unicode scalar value.
    Char(char),
    /// A validated universally unique identifier.
    Uuid(Uuid),
    /// A typed record with fields declared by a model.
    #[allow(dead_code)]
    // Constructed by the native HTTP JSON boundary, which is unavailable in WASI.
    Model {
        /// The model declaration name.
        name: String,
        /// Values indexed by their declared field names.
        fields: std::collections::BTreeMap<String, Value>,
    },
    /// An ordered collection returned by native HTTP store operations.
    #[allow(dead_code)]
    List(Vec<Value>),
    /// An HTTP response value created by the response built-ins.
    Response {
        /// The HTTP status code.
        status: u16,
        /// The JSON or text value sent to the client.
        body: Box<Value>,
        /// User-defined response headers validated by the response helpers.
        headers: Vec<(String, String)>,
    },
    /// The absence of a meaningful value, currently returned by functions.
    Unit,
    /// A stateless provider selected for a dependency contract.
    Provider {
        /// Contract used by the receiving parameter.
        contract: String,
        /// Concrete provider selected by `bind`.
        provider: String,
    },
}

impl std::fmt::Display for Value {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Integer { value, .. } => write!(formatter, "{value}"),
            Self::Double(number) => write!(formatter, "{number}"),
            Self::String(value) => write!(formatter, "{value}"),
            Self::Bool(value) => write!(formatter, "{value}"),
            Self::Char(value) => write!(formatter, "{value}"),
            Self::Uuid(value) => write!(formatter, "{value}"),
            Self::Model { name, .. } => write!(formatter, "{name}"),
            Self::List(values) => write!(formatter, "{} item(s)", values.len()),
            Self::Response { body, .. } => write!(formatter, "{body}"),
            Self::Unit => write!(formatter, "unit"),
            Self::Provider { provider, .. } => write!(formatter, "{provider}"),
        }
    }
}

impl Value {
    /// Returns the built-in Orni type represented by this runtime value.
    pub fn type_name(&self) -> TypeName {
        match self {
            Self::Integer { integer_type, .. } => TypeName::Integer(*integer_type),
            Self::Double(_) => TypeName::Double,
            Self::String(value) => TypeName::String(Some(value.len())),
            Self::Bool(_) => TypeName::Bool,
            Self::Char(_) => TypeName::Char,
            Self::Uuid(_) => TypeName::Uuid,
            Self::Model { name, .. } => TypeName::Model(name.clone()),
            Self::List(_) => TypeName::Response,
            Self::Response { .. } => TypeName::Response,
            Self::Unit => TypeName::Unit,
            Self::Provider { contract, .. } => TypeName::Contract(contract.clone()),
        }
    }
}
