use std::collections::HashMap;

use crate::{
    diagnostics::OrniError,
    syntax::{Expression, IntegerType, Parameter, Statement, Token, TokenKind, TypeName, Value},
};

use super::ExecutionLimits;
#[cfg(not(target_arch = "wasm32"))]
use super::MemoryStores;
use super::environment::Environment;

#[derive(Clone)]
/// A function declaration stored for later invocation.
struct Function {
    parameters: Vec<Parameter>,
    return_type: TypeName,
    body: std::rc::Rc<[Statement]>,
}

/// Describes how execution should continue after a statement.
enum ExecutionFlow {
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
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for ApplicationConfig {
    fn default() -> Self {
        Self {
            name: None,
            port: 3000,
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
        }
    }

    /// Creates an interpreter connected to HTTP-owned in-memory stores.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_stores(limits: ExecutionLimits, stores: MemoryStores) -> Self {
        let mut interpreter = Self::new(limits);
        interpreter.stores = Some(stores);
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
        mut self,
        program: &[Statement],
        handler: &str,
        arguments: Vec<Value>,
    ) -> Result<Value, OrniError> {
        self.register_declarations(program);
        for statement in program {
            if matches!(statement, Statement::Let { .. }) {
                self.execute_statement(statement)?;
            }
        }
        let name = Token::new(TokenKind::Identifier(handler.to_owned()), 1, 1);
        let closing_parenthesis = Token::new(TokenKind::RightParen, 1, 1);
        self.call_values(&name, &closing_parenthesis, arguments)
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
    fn execute_statement(&mut self, statement: &Statement) -> Result<ExecutionFlow, OrniError> {
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

    /// Invokes a named function with evaluated argument values.
    fn call(
        &mut self,
        callee: &Expression,
        closing_parenthesis: &Token,
        arguments: &[Expression],
    ) -> Result<Value, OrniError> {
        let Expression::Variable(name_token) = callee else {
            return Err(
                runtime_error(closing_parenthesis, "only named functions can be called")
                    .with_code("ORNI-E150"),
            );
        };
        let name = identifier_name(name_token);
        if matches!(
            name,
            "response.ok" | "response.created" | "response.not_found" | "response.bad_request"
        ) {
            if arguments.len() != 1 {
                return Err(runtime_error(
                    closing_parenthesis,
                    &format!(
                        "function '{name}' expects 1 argument(s), but received {}",
                        arguments.len()
                    ),
                )
                .with_code("ORNI-E108"));
            }
            let body = self.evaluate(&arguments[0])?;
            return Ok(Value::Response {
                status: match name {
                    "response.created" => 201,
                    "response.not_found" => 404,
                    "response.bad_request" => 400,
                    _ => 200,
                },
                body: Box::new(body),
            });
        }
        if name == "response.no_content" {
            if !arguments.is_empty() {
                return Err(runtime_error(
                    closing_parenthesis,
                    "function 'response.no_content' expects 0 argument(s)",
                )
                .with_code("ORNI-E108"));
            }
            return Ok(Value::Response {
                status: 204,
                body: Box::new(Value::Unit),
            });
        }
        if name == "app.name" {
            let [value] = arguments else {
                return Err(runtime_error(
                    closing_parenthesis,
                    "function 'app.name' expects 1 argument",
                )
                .with_code("ORNI-E108"));
            };
            let Value::String(name) = self.evaluate(value)? else {
                return Err(runtime_error(
                    expression_token(value).unwrap_or(closing_parenthesis),
                    "app.name requires a string",
                )
                .with_code("ORNI-E107"));
            };
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.application.name = Some(name.to_string());
            }
            #[cfg(target_arch = "wasm32")]
            let _ = name;
            return Ok(Value::Unit);
        }
        if name == "app.port" {
            let [value] = arguments else {
                return Err(runtime_error(
                    closing_parenthesis,
                    "function 'app.port' expects 1 argument",
                )
                .with_code("ORNI-E108"));
            };
            let Value::Integer { value: port, .. } = self.evaluate(value)? else {
                return Err(runtime_error(
                    expression_token(value).unwrap_or(closing_parenthesis),
                    "app.port requires an integer",
                )
                .with_code("ORNI-E107"));
            };
            let port = u16::try_from(port)
                .ok()
                .filter(|port| *port > 0)
                .ok_or_else(|| {
                    runtime_error(
                        expression_token(value).unwrap_or(closing_parenthesis),
                        "app.port must be between 1 and 65535",
                    )
                    .with_code("ORNI-E107")
                })?;
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.application.port = port;
            }
            #[cfg(target_arch = "wasm32")]
            let _ = port;
            return Ok(Value::Unit);
        }
        let values = arguments
            .iter()
            .map(|argument| self.evaluate(argument))
            .collect::<Result<Vec<_>, _>>()?;
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(response) = self.store_call(name, closing_parenthesis, &values)? {
            return Ok(response);
        }
        self.call_values(name_token, closing_parenthesis, values)
    }

    /// Performs one CRUD operation against an HTTP-owned in-memory store.
    #[cfg(not(target_arch = "wasm32"))]
    fn store_call(
        &mut self,
        name: &str,
        _token: &Token,
        values: &[Value],
    ) -> Result<Option<Value>, OrniError> {
        let Some((store_name, operation)) = name.split_once('.') else {
            return Ok(None);
        };
        let Some(stores) = &self.stores else {
            return Ok(None);
        };
        let mut stores = stores.borrow_mut();
        let Some(store) = stores.get_mut(store_name) else {
            return Ok(None);
        };
        let response = match operation {
            "list" => Value::Response {
                status: 200,
                body: Box::new(Value::List(store.values.values().cloned().collect())),
            },
            "create" => {
                let input = values
                    .first()
                    .cloned()
                    .expect("type checking validates store arity");
                let Value::Model { fields, .. } = input else {
                    unreachable!("type checking validates create models")
                };
                let id = uuid::Uuid::new_v4();
                let mut fields = fields;
                fields.insert("id".to_owned(), Value::Uuid(id));
                let model = Value::Model {
                    name: store.model.clone(),
                    fields,
                };
                store.values.insert(id, model.clone());
                Value::Response {
                    status: 201,
                    body: Box::new(model),
                }
            }
            "find" => match values
                .first()
                .cloned()
                .expect("type checking validates store arity")
            {
                Value::Uuid(id) => store
                    .values
                    .get(&id)
                    .cloned()
                    .map(|model| Value::Response {
                        status: 200,
                        body: Box::new(model),
                    })
                    .unwrap_or_else(|| Value::Response {
                        status: 404,
                        body: Box::new(Value::String("Not Found".into())),
                    }),
                _ => unreachable!("type checking validates store IDs"),
            },
            "replace" => {
                let id = match values.first() {
                    Some(Value::Uuid(id)) => *id,
                    _ => unreachable!("type checking validates store IDs"),
                };
                let model = values
                    .get(1)
                    .cloned()
                    .expect("type checking validates store arity");
                let Value::Model { fields, .. } = model else {
                    unreachable!("type checking validates replace models")
                };
                let mut fields = fields;
                fields.insert("id".to_owned(), Value::Uuid(id));
                let model = Value::Model {
                    name: store.model.clone(),
                    fields,
                };
                if let std::collections::btree_map::Entry::Occupied(mut entry) =
                    store.values.entry(id)
                {
                    entry.insert(model.clone());
                    Value::Response {
                        status: 200,
                        body: Box::new(model),
                    }
                } else {
                    Value::Response {
                        status: 404,
                        body: Box::new(Value::String("Not Found".into())),
                    }
                }
            }
            "patch" => {
                let id = match values.first() {
                    Some(Value::Uuid(id)) => *id,
                    _ => unreachable!("type checking validates store IDs"),
                };
                let Value::Model { fields, .. } = values
                    .get(1)
                    .cloned()
                    .expect("type checking validates store arity")
                else {
                    unreachable!("type checking validates patch models")
                };
                let Some(Value::Model {
                    fields: current, ..
                }) = store.values.get_mut(&id)
                else {
                    return Ok(Some(Value::Response {
                        status: 404,
                        body: Box::new(Value::String("Not Found".into())),
                    }));
                };
                for (name, value) in fields {
                    current.insert(name, value);
                }
                Value::Response {
                    status: 200,
                    body: Box::new(
                        store
                            .values
                            .get(&id)
                            .cloned()
                            .expect("the stored value exists"),
                    ),
                }
            }
            "delete" => match values.first() {
                Some(Value::Uuid(id)) => {
                    if store.values.remove(id).is_some() {
                        Value::Response {
                            status: 204,
                            body: Box::new(Value::Unit),
                        }
                    } else {
                        Value::Response {
                            status: 404,
                            body: Box::new(Value::String("Not Found".into())),
                        }
                    }
                }
                _ => unreachable!("type checking validates store IDs"),
            },
            _ => return Ok(None),
        };
        Ok(Some(response))
    }

    /// Invokes a named function with values supplied by the embedding runtime.
    fn call_values(
        &mut self,
        name_token: &Token,
        closing_parenthesis: &Token,
        values: Vec<Value>,
    ) -> Result<Value, OrniError> {
        let name = identifier_name(name_token);
        if self.call_depth >= self.limits.max_call_depth {
            return Err(runtime_error(
                name_token,
                &format!(
                    "function call depth exceeds the configured limit of {}",
                    self.limits.max_call_depth
                ),
            )
            .with_code("ORNI-E134"));
        }
        let resolved_name = if self.functions.contains_key(name) {
            name.to_owned()
        } else if let Some((receiver, method)) = name.split_once('.') {
            match self.environment.get(receiver) {
                Some(Value::Provider { provider, .. }) => format!("{provider}.{method}"),
                _ => name.to_owned(),
            }
        } else {
            name.to_owned()
        };
        let function = self.functions.get(&resolved_name).cloned().ok_or_else(|| {
            runtime_error(name_token, &format!("undefined function '{name}'"))
                .with_code("ORNI-E103")
        })?;

        let required_arguments = function
            .parameters
            .iter()
            .filter(|parameter| !matches!(&parameter.type_name, TypeName::Contract(_)))
            .filter(|parameter| parameter.default_value.is_none())
            .count();
        let supplied_parameter_count = function
            .parameters
            .iter()
            .filter(|parameter| !matches!(&parameter.type_name, TypeName::Contract(_)))
            .count();
        if values.len() < required_arguments || values.len() > supplied_parameter_count {
            let expected = if required_arguments == supplied_parameter_count {
                format!("{supplied_parameter_count} argument(s)")
            } else {
                format!(
                    "between {required_arguments} and {} argument(s)",
                    supplied_parameter_count
                )
            };
            return Err(runtime_error(
                closing_parenthesis,
                &format!(
                    "function '{name}' expects {expected}, but received {}",
                    values.len(),
                ),
            )
            .with_code("ORNI-E108"));
        }

        self.call_depth += 1;
        self.environment.enter_function_scope();
        let result = self
            .bind_parameters(&function.parameters, values)
            .and_then(|()| self.execute_function_body(&function.body))
            .and_then(|value| coerce_value(value, Some(function.return_type), closing_parenthesis));
        self.environment.exit_function_scope();
        self.call_depth -= 1;
        result
    }

    /// Binds supplied arguments and evaluates defaults for omitted arguments.
    fn bind_parameters(
        &mut self,
        parameters: &[Parameter],
        values: Vec<Value>,
    ) -> Result<(), OrniError> {
        let mut values = values.into_iter();
        for parameter in parameters {
            let value = match &parameter.type_name {
                TypeName::Contract(contract) => {
                    let provider = self.bindings.get(contract).cloned().ok_or_else(|| {
                        runtime_error(
                            &parameter.name,
                            &format!("contract '{contract}' has no provider binding"),
                        )
                        .with_code("ORNI-E102")
                    })?;
                    Value::Provider {
                        contract: contract.clone(),
                        provider,
                    }
                }
                _ => match values.next() {
                    Some(value) => value,
                    None => self.evaluate(
                        parameter
                            .default_value
                            .as_ref()
                            .expect("the arity check guarantees a default value"),
                    )?,
                },
            };
            let value = coerce_value(value, Some(parameter.type_name.clone()), &parameter.name)?;
            self.define_variable(identifier_name(&parameter.name), value, &parameter.name)?;
        }
        Ok(())
    }

    /// Executes a function body until it finishes or encounters `return`.
    fn execute_function_body(&mut self, body: &[Statement]) -> Result<Value, OrniError> {
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

    /// Applies a binary operator to two already evaluated operands.
    fn binary(&self, left: Value, operator: &Token, right: Value) -> Result<Value, OrniError> {
        match (&operator.kind, left, right) {
            (TokenKind::Plus, Value::String(left), Value::String(right)) => {
                self.concatenate(&left, &right, operator)
            }
            (TokenKind::Plus, Value::String(left), right) => {
                self.concatenate(&left, &right.to_string(), operator)
            }
            (TokenKind::Plus, left, Value::String(right)) => {
                self.concatenate(&left.to_string(), &right, operator)
            }
            (
                _,
                Value::Integer {
                    value: left,
                    integer_type: left_type,
                },
                Value::Integer {
                    value: right,
                    integer_type: right_type,
                },
            ) => self.integer_binary(left, left_type, operator, right, right_type),
            (_, Value::Double(left), Value::Double(right)) => {
                self.double_binary(left, operator, right)
            }
            (_, Value::Integer { value: left, .. }, Value::Double(right)) => {
                self.double_binary(integer_to_double(left, operator)?, operator, right)
            }
            (_, Value::Double(left), Value::Integer { value: right, .. }) => {
                self.double_binary(left, operator, integer_to_double(right, operator)?)
            }
            (TokenKind::EqualEqual, left, right) => Ok(Value::Bool(left == right)),
            (TokenKind::BangEqual, left, right) => Ok(Value::Bool(left != right)),
            (
                TokenKind::Plus
                | TokenKind::Minus
                | TokenKind::Star
                | TokenKind::Slash
                | TokenKind::Greater
                | TokenKind::GreaterEqual
                | TokenKind::Less
                | TokenKind::LessEqual,
                _,
                _,
            ) => Err(runtime_error(
                operator,
                "incompatible types for this operation",
            )),
            _ => unreachable!("binary operator validated by the parser"),
        }
    }

    /// Allocates a concatenated string once and enforces its configured limit.
    fn concatenate(&self, left: &str, right: &str, token: &Token) -> Result<Value, OrniError> {
        let length = left
            .len()
            .checked_add(right.len())
            .ok_or_else(|| runtime_error(token, "string size overflow").with_code("ORNI-E137"))?;
        if length > self.limits.max_string_bytes {
            return Err(runtime_error(
                token,
                &format!(
                    "string requires {length} UTF-8 byte(s), but the runtime limit is {}",
                    self.limits.max_string_bytes
                ),
            )
            .with_code("ORNI-E137"));
        }

        let mut combined = String::with_capacity(length);
        combined.push_str(left);
        combined.push_str(right);
        Ok(Value::String(combined.into()))
    }

    /// Retains a variable only when the active environment has capacity.
    fn define_variable(
        &mut self,
        name: &str,
        value: Value,
        token: &Token,
    ) -> Result<(), OrniError> {
        let replaces_existing = self.environment.current_value(name).is_some();
        if !replaces_existing && self.environment.binding_count() >= self.limits.max_variables {
            return Err(runtime_error(
                token,
                &format!(
                    "variable count exceeds the configured limit of {}",
                    self.limits.max_variables
                ),
            )
            .with_code("ORNI-E138"));
        }

        let value_bytes = match &value {
            Value::String(value) => value.len(),
            _ => 0,
        };
        if value_bytes > self.limits.max_string_bytes {
            return Err(runtime_error(
                token,
                &format!(
                    "string requires {value_bytes} UTF-8 byte(s), but the runtime limit is {}",
                    self.limits.max_string_bytes
                ),
            )
            .with_code("ORNI-E137"));
        }
        let replaced_bytes =
            self.environment
                .current_value(name)
                .map_or(0, |existing| match existing {
                    Value::String(value) => value.len(),
                    _ => 0,
                });
        let retained_bytes = self
            .environment
            .retained_string_bytes()
            .saturating_sub(replaced_bytes)
            .checked_add(value_bytes)
            .ok_or_else(|| {
                runtime_error(token, "retained string size overflow").with_code("ORNI-E135")
            })?;
        if retained_bytes > self.limits.max_retained_string_bytes {
            return Err(runtime_error(
                token,
                &format!(
                    "retained strings exceed the configured limit of {} UTF-8 byte(s)",
                    self.limits.max_retained_string_bytes
                ),
            )
            .with_code("ORNI-E135"));
        }
        self.environment.define(name.to_owned(), value);
        Ok(())
    }

    /// Consumes one unit of execution fuel before performing an operation.
    fn consume_operation(&mut self, token: &Token) -> Result<(), OrniError> {
        if self.operations >= self.limits.max_operations {
            return Err(runtime_error(
                token,
                &format!(
                    "execution exceeds the configured limit of {} operation(s)",
                    self.limits.max_operations
                ),
            )
            .with_code("ORNI-E139"));
        }
        self.operations += 1;
        Ok(())
    }

    /// Applies a checked operation to two integer operands.
    fn integer_binary(
        &self,
        left: i128,
        left_type: IntegerType,
        operator: &Token,
        right: i128,
        right_type: IntegerType,
    ) -> Result<Value, OrniError> {
        let result_type = if left_type == right_type {
            left_type
        } else {
            IntegerType::Int64
        };
        let checked = |value: Option<i128>| {
            value
                .ok_or_else(|| {
                    runtime_error(operator, "integer arithmetic overflow").with_code("ORNI-E132")
                })
                .and_then(|value| checked_integer(value, result_type, operator))
        };
        match operator.kind {
            TokenKind::Plus => checked(left.checked_add(right)),
            TokenKind::Minus => checked(left.checked_sub(right)),
            TokenKind::Star => checked(left.checked_mul(right)),
            TokenKind::Slash if right == 0 => {
                Err(runtime_error(operator, "division by zero").with_code("ORNI-E131"))
            }
            TokenKind::Slash => checked_integer(left / right, result_type, operator),
            TokenKind::Greater => Ok(Value::Bool(left > right)),
            TokenKind::GreaterEqual => Ok(Value::Bool(left >= right)),
            TokenKind::Less => Ok(Value::Bool(left < right)),
            TokenKind::LessEqual => Ok(Value::Bool(left <= right)),
            TokenKind::EqualEqual => Ok(Value::Bool(left == right)),
            TokenKind::BangEqual => Ok(Value::Bool(left != right)),
            _ => unreachable!("operator validated for numeric operands"),
        }
    }

    /// Applies an operation to two double-precision operands.
    fn double_binary(&self, left: f64, operator: &Token, right: f64) -> Result<Value, OrniError> {
        match operator.kind {
            TokenKind::Plus => Ok(Value::Double(left + right)),
            TokenKind::Minus => Ok(Value::Double(left - right)),
            TokenKind::Star => Ok(Value::Double(left * right)),
            TokenKind::Slash if right == 0.0 => {
                Err(runtime_error(operator, "division by zero").with_code("ORNI-E131"))
            }
            TokenKind::Slash => Ok(Value::Double(left / right)),
            TokenKind::Greater => Ok(Value::Bool(left > right)),
            TokenKind::GreaterEqual => Ok(Value::Bool(left >= right)),
            TokenKind::Less => Ok(Value::Bool(left < right)),
            TokenKind::LessEqual => Ok(Value::Bool(left <= right)),
            TokenKind::EqualEqual => Ok(Value::Bool(left == right)),
            TokenKind::BangEqual => Ok(Value::Bool(left != right)),
            _ => unreachable!("operator validated for numeric operands"),
        }
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
        Statement::Block(_) | Statement::Expression(_) => None,
    }
}

/// Finds the source token representing an expression-level operation.
fn expression_token(expression: &Expression) -> Option<&Token> {
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
fn identifier_name(token: &Token) -> &str {
    let TokenKind::Identifier(name) = &token.kind else {
        unreachable!("the parser only produces valid names")
    };
    name
}

/// Creates a runtime error positioned at the provided token.
fn runtime_error(token: &Token, message: &str) -> OrniError {
    OrniError::runtime(token.line, token.column, message).with_token(token)
}

/// Applies checked integer conversions and numeric widening.
fn coerce_value(
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
