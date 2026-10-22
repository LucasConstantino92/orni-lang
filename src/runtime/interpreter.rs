use std::collections::HashMap;

use crate::{
    diagnostics::OrniError,
    syntax::{Expression, IntegerType, Parameter, Statement, Token, TokenKind, TypeName, Value},
};

use super::ExecutionLimits;
use super::environment::Environment;
mod calls;
mod operations;
#[cfg(not(target_arch = "wasm32"))]
mod stores;
#[cfg(not(target_arch = "wasm32"))]
use super::{ApiTestContext, MemoryStores, api_test_request};

#[derive(Clone)]
/// A function declaration stored for later invocation.
struct Function {
    parameters: Vec<Parameter>,
    return_type: TypeName,
    body: std::rc::Rc<[Statement]>,
}

/// Describes how execution should continue after a statement.
pub(super) enum ExecutionFlow {
    /// Continue with the next statement in the current block.
    Continue,
    /// Stop the current function and yield a value.
    Return { keyword: Token, value: Value },
}

/// Startup settings declared by the application entry point.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ApplicationConfig {
    /// Human-readable application name displayed when the server starts.
    pub name: Option<String>,
    /// Local TCP port used by the development server.
    pub port: u16,
    /// Sensitive routes with focused brute-force protection.
    pub protected_routes: Vec<RouteProtection>,
    /// Explicit browser origins allowed to call this local API.
    pub cors_origins: Vec<String>,
    /// Token settings used by routes that require a signed Bearer token.
    pub auth: Option<AuthConfig>,
    /// Exact route paths that require a valid Bearer token.
    pub authenticated_routes: Vec<String>,
    /// Exact route paths that exchange a refresh token for a new access token.
    pub refresh_routes: Vec<String>,
    /// Exact route paths that revoke every in-memory refresh session for a subject.
    pub logout_routes: Vec<String>,
    /// Maximum accepted request body bytes set in the application entry point.
    pub body_limit_bytes: Option<usize>,
    /// Maximum retained cache bytes set in the application entry point.
    pub cache_limit_bytes: Option<usize>,
}

/// Application-owned token settings. The secret never leaves the server process.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AuthConfig {
    /// Secret used to sign and validate tokens.
    pub secret: String,
    /// Maximum access-token lifetime, in milliseconds.
    pub access_duration_milliseconds: u64,
    /// Maximum refresh-token lifetime, in milliseconds.
    pub refresh_duration_milliseconds: u64,
    /// Maximum concurrent in-memory refresh sessions allowed for one subject.
    pub maximum_sessions_per_subject: usize,
}

/// One application-declared login protection policy.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RouteProtection {
    /// Exact route path protected by this policy.
    pub path: String,
    /// Failed authentication responses permitted before a temporary lock.
    pub maximum_attempts: usize,
    /// Lock duration after the failed-attempt threshold is reached, in milliseconds.
    pub lockout_milliseconds: u64,
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for ApplicationConfig {
    fn default() -> Self {
        Self {
            name: None,
            port: 3000,
            protected_routes: Vec::new(),
            cors_origins: Vec::new(),
            auth: None,
            authenticated_routes: Vec::new(),
            refresh_routes: Vec::new(),
            logout_routes: Vec::new(),
            body_limit_bytes: None,
            cache_limit_bytes: None,
        }
    }
}

/// Evaluates an Orni abstract syntax tree and captures printed output.
pub(crate) struct Interpreter {
    environment: Environment,
    functions: HashMap<String, Function>,
    bindings: HashMap<String, String>,
    output: String,
    limits: ExecutionLimits,
    call_depth: usize,
    operations: usize,
    #[cfg(not(target_arch = "wasm32"))]
    stores: Option<MemoryStores>,
    #[cfg(not(target_arch = "wasm32"))]
    application: ApplicationConfig,
    #[cfg(not(target_arch = "wasm32"))]
    api_test: Option<ApiTestContext>,
    /// Optional HTTP resource revision that must still match before a mutation.
    #[cfg(not(target_arch = "wasm32"))]
    expected_store_version: Option<u64>,
    /// Authenticated subject injected by the native HTTP boundary for one request.
    #[cfg(not(target_arch = "wasm32"))]
    authenticated_subject: Option<String>,
    /// Whether the request presented a refresh token rather than an access token.
    #[cfg(not(target_arch = "wasm32"))]
    authenticated_with_refresh_token: bool,
}

impl Interpreter {
    /// Creates an interpreter with empty global state and output.
    pub fn new(limits: ExecutionLimits) -> Self {
        Self {
            environment: Environment::new(),
            functions: HashMap::new(),
            bindings: HashMap::new(),
            output: String::new(),
            limits,
            call_depth: 0,
            operations: 0,
            #[cfg(not(target_arch = "wasm32"))]
            stores: None,
            #[cfg(not(target_arch = "wasm32"))]
            application: ApplicationConfig::default(),
            #[cfg(not(target_arch = "wasm32"))]
            api_test: None,
            #[cfg(not(target_arch = "wasm32"))]
            expected_store_version: None,
            #[cfg(not(target_arch = "wasm32"))]
            authenticated_subject: None,
            #[cfg(not(target_arch = "wasm32"))]
            authenticated_with_refresh_token: false,
        }
    }

    /// Creates an interpreter connected to HTTP-owned in-memory stores.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_stores(limits: ExecutionLimits, stores: MemoryStores) -> Self {
        let mut interpreter = Self::new(limits);
        interpreter.stores = Some(stores);
        interpreter
    }

    /// Creates an interpreter whose next store mutation must match an HTTP ETag revision.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_stores_and_expected_version(
        limits: ExecutionLimits,
        stores: MemoryStores,
        expected_store_version: Option<u64>,
    ) -> Self {
        let mut interpreter = Self::with_stores(limits, stores);
        interpreter.expected_store_version = expected_store_version;
        interpreter
    }

    /// Creates an interpreter for one HTTP request with an already-validated subject.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_http_context(
        limits: ExecutionLimits,
        stores: MemoryStores,
        expected_store_version: Option<u64>,
        auth: Option<AuthConfig>,
        authenticated_subject: Option<String>,
        authenticated_with_refresh_token: bool,
    ) -> Self {
        let mut interpreter =
            Self::with_stores_and_expected_version(limits, stores, expected_store_version);
        interpreter.application.auth = auth;
        interpreter.authenticated_subject = authenticated_subject;
        interpreter.authenticated_with_refresh_token = authenticated_with_refresh_token;
        interpreter
    }

    /// Creates an interpreter that can call the application's routes from a test file.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_api_test(limits: ExecutionLimits, api_test: ApiTestContext) -> Self {
        let mut interpreter = Self::new(limits);
        interpreter.api_test = Some(api_test);
        interpreter
    }

    /// Executes a complete program and returns its captured output.
    pub fn execute(mut self, program: &[Statement]) -> Result<String, OrniError> {
        self.register_declarations(program);
        for statement in program {
            if let ExecutionFlow::Return { keyword, .. } = self.execute_statement(statement)? {
                return Err(
                    runtime_error(&keyword, "return can only be used inside a function")
                        .with_code("ORNI-E118"),
                );
            }
        }
        self.call_application_main()?;
        Ok(self.output)
    }

    /// Runs an optional zero-argument application entry point once.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn execute_main(
        mut self,
        program: &[Statement],
    ) -> Result<(String, ApplicationConfig), OrniError> {
        self.register_declarations(program);
        for statement in program {
            if matches!(statement, Statement::Let { .. }) {
                self.execute_statement(statement)?;
            }
        }
        self.call_application_main()?;
        Ok((self.output, self.application))
    }

    /// Calls a validated route handler and returns its response value.
    #[cfg(any(not(target_arch = "wasm32"), test))]
    pub fn execute_handler(
        self,
        program: &[Statement],
        handler: &str,
        arguments: Vec<Value>,
    ) -> Result<Value, OrniError> {
        self.execute_handler_with_optional_arguments(
            program,
            handler,
            arguments.into_iter().map(Some).collect(),
        )
    }

    /// Calls a route handler while preserving omitted optional argument slots.
    #[cfg(any(not(target_arch = "wasm32"), test))]
    pub fn execute_handler_with_optional_arguments(
        mut self,
        program: &[Statement],
        handler: &str,
        arguments: Vec<Option<Value>>,
    ) -> Result<Value, OrniError> {
        self.register_declarations(program);
        for statement in program {
            if matches!(statement, Statement::Let { .. }) {
                self.execute_statement(statement)?;
            }
        }
        let name = Token::new(TokenKind::Identifier(handler.to_owned()), 1, 1);
        let closing_parenthesis = Token::new(TokenKind::RightParen, 1, 1);
        self.call_optional_values(&name, &closing_parenthesis, arguments)
    }

    fn register_declarations(&mut self, program: &[Statement]) {
        for statement in program {
            match statement {
                Statement::Function {
                    name,
                    parameters,
                    return_type,
                    body,
                    ..
                } => {
                    self.functions.insert(
                        identifier_name(name).to_owned(),
                        Function {
                            parameters: parameters.clone(),
                            return_type: return_type.clone(),
                            body: body.clone().into(),
                        },
                    );
                }
                Statement::Provider {
                    name,
                    dependencies,
                    methods,
                    ..
                } => {
                    let provider_name = identifier_name(name);
                    for method in methods {
                        let Statement::Function {
                            name,
                            parameters,
                            return_type,
                            body,
                        } = method
                        else {
                            unreachable!("providers contain only functions")
                        };
                        let mut injected_parameters = dependencies.clone();
                        injected_parameters.extend(parameters.clone());
                        self.functions.insert(
                            format!("{provider_name}.{}", identifier_name(name)),
                            Function {
                                parameters: injected_parameters,
                                return_type: return_type.clone(),
                                body: body.clone().into(),
                            },
                        );
                    }
                }
                Statement::Bind {
                    contract, provider, ..
                } => {
                    self.bindings.insert(
                        identifier_name(contract).to_owned(),
                        identifier_name(provider).to_owned(),
                    );
                }
                _ => {}
            }
        }
    }

    /// Invokes the optional validated application entry point.
    fn call_application_main(&mut self) -> Result<(), OrniError> {
        if self.functions.contains_key("main") {
            let name = Token::new(TokenKind::Identifier("main".to_owned()), 1, 1);
            let closing_parenthesis = Token::new(TokenKind::RightParen, 1, 1);
            self.call_values(&name, &closing_parenthesis, Vec::new())?;
        }
        Ok(())
    }

    /// Executes one statement and updates interpreter state when necessary.
    pub(super) fn execute_statement(
        &mut self,
        statement: &Statement,
    ) -> Result<ExecutionFlow, OrniError> {
        if let Some(token) = statement_token(statement) {
            self.consume_operation(token)?;
        }
        match statement {
            Statement::Import { .. } => {
                unreachable!("module imports are resolved before interpretation")
            }
            Statement::Contract { .. }
            | Statement::Model { .. }
            | Statement::Store { .. }
            | Statement::Provider { .. }
            | Statement::Bind { .. }
            | Statement::Route { .. } => {}
            Statement::Let {
                name,
                declared_type,
                value,
            } => {
                let identifier = identifier_name(name);
                let value = coerce_value(self.evaluate(value)?, declared_type.clone(), name)?;
                self.define_variable(identifier, value, name)?;
            }
            Statement::Function {
                name,
                parameters,
                return_type,
                body,
                ..
            } => {
                self.functions.insert(
                    identifier_name(name).to_owned(),
                    Function {
                        parameters: parameters.clone(),
                        return_type: return_type.clone(),
                        body: body.clone().into(),
                    },
                );
            }
            Statement::Print {
                keyword,
                expression,
            } => {
                let value = self.evaluate(expression)?;
                let rendered = value.to_string();
                let next_length = self
                    .output
                    .len()
                    .checked_add(rendered.len())
                    .and_then(|length| length.checked_add(1))
                    .ok_or_else(|| {
                        runtime_error(keyword, "output size overflow").with_code("ORNI-E136")
                    })?;
                if next_length > self.limits.max_output_bytes {
                    return Err(runtime_error(
                        keyword,
                        &format!(
                            "output exceeds the configured limit of {} UTF-8 byte(s)",
                            self.limits.max_output_bytes
                        ),
                    )
                    .with_code("ORNI-E136"));
                }
                self.output.push_str(&rendered);
                self.output.push('\n');
            }
            Statement::Return { keyword, value } => {
                let value = value
                    .as_ref()
                    .map(|expression| self.evaluate(expression))
                    .transpose()?
                    .unwrap_or(Value::Unit);
                return Ok(ExecutionFlow::Return {
                    keyword: keyword.clone(),
                    value,
                });
            }
            Statement::If {
                keyword,
                condition,
                then_branch,
                else_branch,
            } => match self.evaluate(condition)? {
                Value::Bool(true) => return self.execute_statement(then_branch),
                Value::Bool(false) => {
                    if let Some(else_branch) = else_branch {
                        return self.execute_statement(else_branch);
                    }
                }
                _ => {
                    return Err(runtime_error(keyword, "if condition must be a boolean"));
                }
            },
            Statement::Block(statements) => return self.execute_block(statements),
            Statement::Expression(expression) => {
                self.evaluate(expression)?;
            }
            Statement::Assert {
                keyword,
                expression,
            } => {
                if !matches!(self.evaluate(expression)?, Value::Bool(true)) {
                    return Err(runtime_error(keyword, "assertion failed").with_code("ORNI-E152"));
                }
            }
            Statement::Test {
                keyword,
                name,
                body,
            } => {
                let TokenKind::String(name) = &name.kind else {
                    unreachable!()
                };
                match self.execute_block(body) {
                    Ok(_) => {
                        self.output.push_str(&format!("PASS {name}\n"));
                    }
                    Err(error) => {
                        return Err(runtime_error(
                            keyword,
                            &format!("test '{name}' failed: {error}"),
                        )
                        .with_code("ORNI-E152"));
                    }
                }
            }
        }
        Ok(ExecutionFlow::Continue)
    }

    /// Evaluates one expression into a runtime value.
    fn evaluate(&mut self, expression: &Expression) -> Result<Value, OrniError> {
        if let Some(token) = expression_token(expression) {
            self.consume_operation(token)?;
        }
        match expression {
            Expression::Literal(value) => Ok(value.clone()),
            Expression::Grouping(expression) => self.evaluate(expression),
            Expression::Variable(token) => {
                let name = identifier_name(token);
                self.environment.get(name).cloned().ok_or_else(|| {
                    runtime_error(token, &format!("undefined variable '{name}'"))
                        .with_code("ORNI-E103")
                })
            }
            Expression::Unary { operator, right } => {
                let right = self.evaluate(right)?;
                match (&operator.kind, right) {
                    (
                        TokenKind::Minus,
                        Value::Integer {
                            value,
                            integer_type,
                        },
                    ) => checked_integer(-value, integer_type, operator),
                    (TokenKind::Minus, Value::Double(number)) => Ok(Value::Double(-number)),
                    (TokenKind::Bang, Value::Bool(value)) => Ok(Value::Bool(!value)),
                    (TokenKind::Minus, _) => {
                        Err(runtime_error(operator, "'-' only accepts numbers"))
                    }
                    (TokenKind::Bang, _) => {
                        Err(runtime_error(operator, "'!' only accepts booleans"))
                    }
                    _ => unreachable!("unary operator validated by the parser"),
                }
            }
            Expression::Binary {
                left,
                operator,
                right,
            } => {
                let left = self.evaluate(left)?;
                let right = self.evaluate(right)?;
                self.binary(left, operator, right)
            }
            Expression::Call {
                callee,
                closing_parenthesis,
                arguments,
            } => self.call(callee, closing_parenthesis, arguments),
        }
    }

    /// Executes a function body until it finishes or encounters `return`.
    pub(super) fn execute_function_body(&mut self, body: &[Statement]) -> Result<Value, OrniError> {
        for statement in body {
            match self.execute_statement(statement)? {
                ExecutionFlow::Continue => {}
                ExecutionFlow::Return { value, .. } => return Ok(value),
            }
        }
        Ok(Value::Unit)
    }

    /// Executes statements in a temporary nested lexical scope.
    fn execute_block(&mut self, statements: &[Statement]) -> Result<ExecutionFlow, OrniError> {
        self.environment.enter_block_scope();
        let result = self.execute_statements(statements);
        self.environment.exit_block_scope();
        result
    }

    /// Executes statements until the block ends or one of them returns.
    fn execute_statements(&mut self, statements: &[Statement]) -> Result<ExecutionFlow, OrniError> {
        for statement in statements {
            match self.execute_statement(statement)? {
                ExecutionFlow::Continue => {}
                returned @ ExecutionFlow::Return { .. } => return Ok(returned),
            }
        }
        Ok(ExecutionFlow::Continue)
    }
}

/// Finds the source token representing a statement-level operation.
fn statement_token(statement: &Statement) -> Option<&Token> {
    match statement {
        Statement::Import { keyword, .. }
        | Statement::Bind { keyword, .. }
        | Statement::Route { keyword, .. } => Some(keyword),
        Statement::Let { name, .. }
        | Statement::Function { name, .. }
        | Statement::Contract { name, .. }
        | Statement::Model { name, .. }
        | Statement::Store { name, .. }
        | Statement::Provider { name, .. } => Some(name),
        Statement::Print { keyword, .. }
        | Statement::Return { keyword, .. }
        | Statement::If { keyword, .. }
        | Statement::Assert { keyword, .. } => Some(keyword),
        Statement::Test { keyword, .. } => Some(keyword),
        Statement::Block(_) | Statement::Expression(_) => None,
    }
}

/// Rejects response headers that could corrupt HTTP framing or contain unsafe bytes.
pub(super) fn validate_response_header(
    name: &str,
    value: &str,
    token: &Token,
) -> Result<(), OrniError> {
    if name.is_empty()
        || !name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
    {
        return Err(
            runtime_error(token, "response header name contains invalid characters")
                .with_code("ORNI-E107"),
        );
    }
    if matches!(
        name.to_ascii_lowercase().as_str(),
        "connection" | "content-length" | "transfer-encoding"
    ) {
        return Err(runtime_error(
            token,
            "response.header cannot override HTTP framing headers",
        )
        .with_code("ORNI-E107"));
    }
    if value
        .bytes()
        .any(|byte| byte == b'\r' || byte == b'\n' || byte.is_ascii_control() && byte != b'\t')
    {
        return Err(
            runtime_error(token, "response header value contains invalid characters")
                .with_code("ORNI-E107"),
        );
    }
    Ok(())
}

/// Finds the source token representing an expression-level operation.
pub(super) fn expression_token(expression: &Expression) -> Option<&Token> {
    match expression {
        Expression::Variable(token) => Some(token),
        Expression::Unary { operator, .. } | Expression::Binary { operator, .. } => Some(operator),
        Expression::Call {
            closing_parenthesis,
            ..
        } => Some(closing_parenthesis),
        Expression::Grouping(expression) => expression_token(expression),
        Expression::Literal(_) => None,
    }
}

/// Extracts an identifier name from a parser-validated token.
pub(super) fn identifier_name(token: &Token) -> &str {
    let TokenKind::Identifier(name) = &token.kind else {
        unreachable!("the parser only produces valid names")
    };
    name
}

/// Creates a runtime error positioned at the provided token.
pub(super) fn runtime_error(token: &Token, message: &str) -> OrniError {
    OrniError::runtime(token.line, token.column, message).with_token(token)
}

/// Applies checked integer conversions and numeric widening.
pub(super) fn coerce_value(
    value: Value,
    expected: Option<TypeName>,
    token: &Token,
) -> Result<Value, OrniError> {
    match (value, expected) {
        (Value::Integer { value, .. }, Some(TypeName::Double)) => {
            Ok(Value::Double(integer_to_double(value, token)?))
        }
        (Value::Integer { value, .. }, Some(TypeName::Integer(integer_type))) => {
            checked_integer(value, integer_type, token)
        }
        (Value::String(value), Some(TypeName::String(Some(max_bytes))))
            if value.len() > max_bytes =>
        {
            Err(runtime_error(
                token,
                &format!(
                    "string requires {} UTF-8 byte(s), but allows at most {max_bytes}",
                    value.len()
                ),
            )
            .with_code("ORNI-E137"))
        }
        (Value::String(value), Some(TypeName::Uuid)) => uuid::Uuid::parse_str(&value)
            .map(Value::Uuid)
            .map_err(|_| runtime_error(token, "value is not a valid UUID").with_code("ORNI-E107")),
        (value, _) => Ok(value),
    }
}

/// Converts an integer only when a `double` can represent it exactly.
fn integer_to_double(value: i128, token: &Token) -> Result<f64, OrniError> {
    let converted = value as f64;
    if converted as i128 == value {
        Ok(converted)
    } else {
        Err(runtime_error(
            token,
            &format!("value {value} cannot be represented exactly as double"),
        )
        .with_code("ORNI-E133"))
    }
}

/// Builds an integer value only when it fits its selected range.
fn checked_integer(
    value: i128,
    integer_type: IntegerType,
    token: &Token,
) -> Result<Value, OrniError> {
    if integer_type.contains(value) {
        Ok(Value::Integer {
            value,
            integer_type,
        })
    } else {
        Err(runtime_error(
            token,
            &format!("value {value} is out of range for {integer_type}"),
        )
        .with_code("ORNI-E104"))
    }
}
