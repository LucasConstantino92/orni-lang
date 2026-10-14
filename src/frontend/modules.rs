use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Component, Path, PathBuf},
};

use crate::{
    diagnostics::{OrniDiagnostic, OrniError},
    syntax::{Expression, Statement, Token, TokenKind},
};

use super::{Lexer, Parser};

/// Loads a file-based Orni program and resolves its module graph.
pub(crate) struct ModuleLoader {
    loaded: HashMap<PathBuf, String>,
    exports: HashMap<PathBuf, HashSet<String>>,
    visiting: Vec<PathBuf>,
    diagnostics: Vec<OrniDiagnostic>,
}

impl ModuleLoader {
    /// Creates a loader with an empty module graph.
    pub fn new() -> Self {
        Self {
            loaded: HashMap::new(),
            exports: HashMap::new(),
            visiting: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    /// Loads an entry file and recursively expands its imports.
    pub fn load(mut self, entry: &Path) -> Result<Vec<Statement>, OrniError> {
        self.load_module(&normalize(entry), None, true, None)
    }

    /// Loads an entry file and returns non-fatal diagnostics with its program.
    pub fn load_with_diagnostics(
        mut self,
        entry: &Path,
    ) -> Result<(Vec<Statement>, Vec<OrniDiagnostic>), OrniError> {
        let program = self.load_module(&normalize(entry), None, true, None)?;
        Ok((program, self.diagnostics))
    }

    /// Loads one file as a reusable module rather than an executable entry point.
    pub fn load_library(mut self, module: &Path) -> Result<Vec<Statement>, OrniError> {
        self.load_module(&normalize(module), Some("__module"), false, None)
    }

    /// Loads a reusable module and returns its non-fatal diagnostics.
    pub fn load_library_with_diagnostics(
        mut self,
        module: &Path,
    ) -> Result<(Vec<Statement>, Vec<OrniDiagnostic>), OrniError> {
        let program = self.load_module(&normalize(module), Some("__module"), false, None)?;
        Ok((program, self.diagnostics))
    }

    fn load_module(
        &mut self,
        path: &Path,
        namespace: Option<&str>,
        is_entry: bool,
        import_token: Option<&Token>,
    ) -> Result<Vec<Statement>, OrniError> {
        let path = normalize(path);
        if self.visiting.contains(&path) {
            let chain = self
                .visiting
                .iter()
                .chain(std::iter::once(&path))
                .map(|item| item.display().to_string())
                .collect::<Vec<_>>()
                .join(" -> ");
            return Err(module_error(
                import_token,
                format!("circular import detected: {chain}"),
            ));
        }
        if let Some(previous_namespace) = self.loaded.get(&path) {
            let requested_namespace = namespace.unwrap_or("");
            if previous_namespace == requested_namespace {
                return Ok(Vec::new());
            }
            return Err(module_error(
                import_token,
                format!(
                    "module '{}' is already imported as '{previous_namespace}'",
                    path.display()
                ),
            ));
        }

        let source = fs::read_to_string(&path).map_err(|error| {
            module_error(
                import_token,
                format!("could not read module '{}': {error}", path.display()),
            )
        })?;
        let tokens = Lexer::new(&source)
            .with_source_name(path.display().to_string())
            .scan_tokens()
            .map_err(|error| error.with_source(&path))?;
        let parsed = Parser::new(tokens)
            .parse()
            .map_err(|error| error.with_source(&path))?;

        if !is_entry {
            for statement in &parsed {
                if !matches!(
                    statement,
                    Statement::Import { .. } | Statement::Function { .. } | Statement::Let { .. }
                ) {
                    let (line, column) = statement_token(statement)
                        .map_or((1, 1), |token| (token.line, token.column));
                    return Err(OrniError::module(
                        line,
                        column,
                        "imported modules may only contain imports, functions, and module variables",
                    )
                    .with_source(&path));
                }
            }
        }

        let (module_functions, module_variables) = collect_module_declarations(&parsed, &path)?;

        self.visiting.push(path.clone());
        let result = (|| {
            let mut program = Vec::new();
            let mut aliases = HashMap::new();
            let mut selected_imports = HashMap::new();

            for statement in &parsed {
                let Statement::Import {
                    keyword,
                    path: segments,
                    alias,
                    members,
                } = statement
                else {
                    continue;
                };
                let local_alias = alias.as_ref().map(identifier_name).unwrap_or_else(|| {
                    identifier_name(segments.last().expect("an import has a path"))
                });
                if members.is_none() && aliases.contains_key(local_alias) {
                    return Err(OrniError::module(
                        keyword.line,
                        keyword.column,
                        format!("module namespace '{local_alias}' is already declared"),
                    )
                    .with_source(&path));
                }
                if members.is_none()
                    && (module_functions.contains(local_alias)
                        || module_variables.contains(local_alias)
                        || selected_imports.contains_key(local_alias))
                {
                    return Err(OrniError::module(
                        keyword.line,
                        keyword.column,
                        format!("module namespace '{local_alias}' conflicts with a declaration"),
                    )
                    .with_source(&path));
                }
                if members.is_none() && !namespace_is_used(&parsed, local_alias) {
                    let mut import_span = segments
                        .first()
                        .expect("an import has at least one path segment")
                        .clone();
                    let final_segment = segments
                        .last()
                        .expect("an import has at least one path segment");
                    import_span.end_line = final_segment.end_line;
                    import_span.end_column = final_segment.end_column;
                    self.diagnostics.push(OrniDiagnostic::warning(
                        &import_span,
                        "ORNI-W001",
                        format!("module namespace '{local_alias}' is imported but never used"),
                    ));
                }
                let child_namespace = namespace.map_or_else(
                    || local_alias.to_owned(),
                    |namespace| format!("{namespace}.{local_alias}"),
                );
                let imported_path = resolve_import(&path, segments);
                let imported_program =
                    self.load_module(&imported_path, Some(&child_namespace), false, Some(keyword))?;
                if let Some(members) = members {
                    let exports = self
                        .exports
                        .get(&imported_path)
                        .expect("a successfully loaded module records its exports");
                    for member in members {
                        let member_name = identifier_name(member);
                        if member_name.starts_with('_') {
                            return Err(OrniError::module(
                                member.line,
                                member.column,
                                format!(
                                    "private member '{member_name}' cannot be imported directly"
                                ),
                            )
                            .with_token(member));
                        }
                        if !exports.contains(member_name) {
                            return Err(OrniError::module(
                                member.line,
                                member.column,
                                format!(
                                    "module '{}' has no public member named '{member_name}'",
                                    imported_path.display()
                                ),
                            )
                            .with_token(member));
                        }
                        if module_functions.contains(member_name)
                            || module_variables.contains(member_name)
                            || aliases.contains_key(member_name)
                            || selected_imports.contains_key(member_name)
                        {
                            return Err(OrniError::module(
                                member.line,
                                member.column,
                                format!(
                                    "selectively imported member '{member_name}' conflicts with a declaration"
                                ),
                            )
                            .with_token(member));
                        }
                        if !name_is_used(&parsed, member_name) {
                            self.diagnostics.push(OrniDiagnostic::warning(
                                member,
                                "ORNI-W002",
                                format!(
                                    "selectively imported member '{member_name}' is never used"
                                ),
                            ));
                        }
                        selected_imports.insert(
                            member_name.to_owned(),
                            format!("{child_namespace}.{member_name}"),
                        );
                    }
                } else {
                    aliases.insert(local_alias.to_owned(), child_namespace);
                }
                program.extend(imported_program);
            }

            let context = TransformContext {
                namespace,
                aliases: &aliases,
                selected_imports: &selected_imports,
                module_functions: &module_functions,
                module_variables: &module_variables,
            };
            for mut statement in parsed {
                if matches!(statement, Statement::Import { .. }) {
                    continue;
                }
                transform_statement(&mut statement, &context, &mut HashSet::new(), true)?;
                program.push(statement);
            }
            Ok(program)
        })();
        self.visiting.pop();
        if result.is_ok() {
            self.exports.insert(
                path.clone(),
                module_functions
                    .iter()
                    .chain(&module_variables)
                    .cloned()
                    .collect(),
            );
            self.loaded.insert(path, namespace.unwrap_or("").to_owned());
        }
        result
    }
}

/// Immutable module information shared while names are rewritten.
struct TransformContext<'a> {
    namespace: Option<&'a str>,
    aliases: &'a HashMap<String, String>,
    selected_imports: &'a HashMap<String, String>,
    module_functions: &'a HashSet<String>,
    module_variables: &'a HashSet<String>,
}

fn transform_statement(
    statement: &mut Statement,
    context: &TransformContext<'_>,
    locals: &mut HashSet<String>,
    module_level: bool,
) -> Result<(), OrniError> {
    match statement {
        Statement::Import { .. } => unreachable!("imports are removed before transformation"),
        Statement::Let { name, value, .. } => {
            transform_expression(value, context, locals)?;
            if module_level && let Some(namespace) = context.namespace {
                qualify_token(name, namespace);
            } else {
                locals.insert(identifier_name(name).to_owned());
            }
        }
        Statement::Function {
            name,
            parameters,
            body,
            ..
        } => {
            let mut function_locals = parameters
                .iter()
                .map(|parameter| identifier_name(&parameter.name).to_owned())
                .collect::<HashSet<_>>();
            for parameter in parameters {
                if let Some(default) = &mut parameter.default_value {
                    transform_expression(default, context, &function_locals)?;
                }
            }
            for statement in body {
                transform_statement(statement, context, &mut function_locals, false)?;
            }
            if module_level && let Some(namespace) = context.namespace {
                qualify_token(name, namespace);
            }
        }
        Statement::Print { expression, .. } | Statement::Expression(expression) => {
            transform_expression(expression, context, locals)?;
        }
        Statement::Return { value, .. } => {
            if let Some(value) = value {
                transform_expression(value, context, locals)?;
            }
        }
        Statement::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            transform_expression(condition, context, locals)?;
            let mut then_locals = locals.clone();
            transform_statement(then_branch, context, &mut then_locals, false)?;
            if let Some(else_branch) = else_branch {
                let mut else_locals = locals.clone();
                transform_statement(else_branch, context, &mut else_locals, false)?;
            }
        }
        Statement::Block(statements) => {
            let mut block_locals = locals.clone();
            for statement in statements {
                transform_statement(statement, context, &mut block_locals, false)?;
            }
        }
    }
    Ok(())
}

fn transform_expression(
    expression: &mut Expression,
    context: &TransformContext<'_>,
    locals: &HashSet<String>,
) -> Result<(), OrniError> {
    match expression {
        Expression::Literal(_) => {}
        Expression::Grouping(expression) => transform_expression(expression, context, locals)?,
        Expression::Variable(token) => {
            let name = identifier_name(token).to_owned();
            if let Some((prefix, member)) = name.split_once('.') {
                if let Some(target_namespace) = context.aliases.get(prefix) {
                    if member.split('.').any(|part| part.starts_with('_')) {
                        return Err(OrniError::module(
                            token.line,
                            token.column,
                            format!(
                                "private member '{member}' cannot be accessed outside its module"
                            ),
                        )
                        .with_token(token));
                    }
                    set_identifier(token, format!("{target_namespace}.{member}"));
                }
            } else if !locals.contains(&name)
                && (context.module_functions.contains(&name)
                    || context.module_variables.contains(&name))
                && let Some(namespace) = context.namespace
            {
                set_identifier(token, format!("{namespace}.{name}"));
            } else if !locals.contains(&name)
                && let Some(imported_name) = context.selected_imports.get(&name)
            {
                set_identifier(token, imported_name.clone());
            }
        }
        Expression::Unary { right, .. } => transform_expression(right, context, locals)?,
        Expression::Binary { left, right, .. } => {
            transform_expression(left, context, locals)?;
            transform_expression(right, context, locals)?;
        }
        Expression::Call {
            callee, arguments, ..
        } => {
            transform_expression(callee, context, locals)?;
            for argument in arguments {
                transform_expression(argument, context, locals)?;
            }
        }
    }
    Ok(())
}

/// Resolves `services.greeting` beside its importing file.
fn resolve_import(importer: &Path, segments: &[Token]) -> PathBuf {
    let mut path = importer
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    for segment in segments {
        path.push(identifier_name(segment));
    }
    path.set_extension("orni");
    normalize(&path)
}

fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn qualify_token(token: &mut Token, namespace: &str) {
    let name = identifier_name(token).to_owned();
    set_identifier(token, format!("{namespace}.{name}"));
}

fn set_identifier(token: &mut Token, name: String) {
    token.kind = TokenKind::Identifier(name);
}

fn identifier_name(token: &Token) -> &str {
    let TokenKind::Identifier(name) = &token.kind else {
        unreachable!("module names contain only parser-validated identifiers")
    };
    name
}

fn statement_token(statement: &Statement) -> Option<&Token> {
    match statement {
        Statement::Let { name, .. } | Statement::Function { name, .. } => Some(name),
        Statement::Import { keyword, .. }
        | Statement::Print { keyword, .. }
        | Statement::Return { keyword, .. }
        | Statement::If { keyword, .. } => Some(keyword),
        Statement::Block(_) | Statement::Expression(_) => None,
    }
}

fn module_error(token: Option<&Token>, message: impl Into<String>) -> OrniError {
    let message = message.into();
    match token {
        Some(token) => OrniError::module(token.line, token.column, message).with_token(token),
        None => OrniError::module(1, 1, message),
    }
}

fn collect_module_declarations(
    statements: &[Statement],
    source_path: &Path,
) -> Result<(HashSet<String>, HashSet<String>), OrniError> {
    let mut functions = HashSet::new();
    let mut variables = HashSet::new();
    let mut declarations = HashSet::new();

    for statement in statements {
        let (name, token, destination) = match statement {
            Statement::Function { name, .. } => (identifier_name(name), name, &mut functions),
            Statement::Let { name, .. } => (identifier_name(name), name, &mut variables),
            _ => continue,
        };
        if !declarations.insert(name.to_owned()) {
            return Err(OrniError::module(
                token.line,
                token.column,
                format!("module member '{name}' is already declared"),
            )
            .with_token(token)
            .with_source(source_path));
        }
        destination.insert(name.to_owned());
    }

    Ok((functions, variables))
}

fn namespace_is_used(statements: &[Statement], namespace: &str) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Import { .. } => false,
        Statement::Let { value, .. } => expression_uses_namespace(value, namespace),
        Statement::Function {
            parameters, body, ..
        } => {
            parameters.iter().any(|parameter| {
                parameter
                    .default_value
                    .as_ref()
                    .is_some_and(|value| expression_uses_namespace(value, namespace))
            }) || namespace_is_used(body, namespace)
        }
        Statement::Print { expression, .. } | Statement::Expression(expression) => {
            expression_uses_namespace(expression, namespace)
        }
        Statement::Return { value, .. } => value
            .as_ref()
            .is_some_and(|value| expression_uses_namespace(value, namespace)),
        Statement::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            expression_uses_namespace(condition, namespace)
                || namespace_is_used(std::slice::from_ref(then_branch.as_ref()), namespace)
                || else_branch.as_ref().is_some_and(|branch| {
                    namespace_is_used(std::slice::from_ref(branch.as_ref()), namespace)
                })
        }
        Statement::Block(statements) => namespace_is_used(statements, namespace),
    })
}

fn expression_uses_namespace(expression: &Expression, namespace: &str) -> bool {
    match expression {
        Expression::Literal(_) => false,
        Expression::Variable(token) => identifier_name(token)
            .strip_prefix(namespace)
            .is_some_and(|suffix| suffix.starts_with('.')),
        Expression::Unary { right, .. } | Expression::Grouping(right) => {
            expression_uses_namespace(right, namespace)
        }
        Expression::Binary { left, right, .. } => {
            expression_uses_namespace(left, namespace)
                || expression_uses_namespace(right, namespace)
        }
        Expression::Call {
            callee, arguments, ..
        } => {
            expression_uses_namespace(callee, namespace)
                || arguments
                    .iter()
                    .any(|argument| expression_uses_namespace(argument, namespace))
        }
    }
}

fn name_is_used(statements: &[Statement], name: &str) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Import { .. } => false,
        Statement::Let { value, .. } => expression_uses_name(value, name),
        Statement::Function {
            parameters, body, ..
        } => {
            parameters.iter().any(|parameter| {
                parameter
                    .default_value
                    .as_ref()
                    .is_some_and(|value| expression_uses_name(value, name))
            }) || name_is_used(body, name)
        }
        Statement::Print { expression, .. } | Statement::Expression(expression) => {
            expression_uses_name(expression, name)
        }
        Statement::Return { value, .. } => value
            .as_ref()
            .is_some_and(|value| expression_uses_name(value, name)),
        Statement::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            expression_uses_name(condition, name)
                || name_is_used(std::slice::from_ref(then_branch.as_ref()), name)
                || else_branch
                    .as_ref()
                    .is_some_and(|branch| name_is_used(std::slice::from_ref(branch.as_ref()), name))
        }
        Statement::Block(statements) => name_is_used(statements, name),
    })
}

fn expression_uses_name(expression: &Expression, name: &str) -> bool {
    match expression {
        Expression::Literal(_) => false,
        Expression::Variable(token) => identifier_name(token) == name,
        Expression::Unary { right, .. } | Expression::Grouping(right) => {
            expression_uses_name(right, name)
        }
        Expression::Binary { left, right, .. } => {
            expression_uses_name(left, name) || expression_uses_name(right, name)
        }
        Expression::Call {
            callee, arguments, ..
        } => {
            expression_uses_name(callee, name)
                || arguments
                    .iter()
                    .any(|argument| expression_uses_name(argument, name))
        }
    }
}
