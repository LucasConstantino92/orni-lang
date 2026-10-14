use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Component, Path, PathBuf},
};

use crate::{
    Project,
    diagnostics::{OrniDiagnostic, OrniError},
    syntax::{Expression, Statement, Token, TokenKind, TypeName},
};

use super::{Lexer, Parser};

/// Loads a file-based Orni program and resolves its module graph.
pub(crate) struct ModuleLoader {
    loaded: HashMap<PathBuf, String>,
    exports: HashMap<PathBuf, HashSet<String>>,
    visiting: Vec<PathBuf>,
    diagnostics: Vec<OrniDiagnostic>,
    project_root: Option<PathBuf>,
    module_root: Option<PathBuf>,
    source_bytes: u64,
    tokens: usize,
    source_overrides: HashMap<PathBuf, String>,
}

impl ModuleLoader {
    /// Creates a loader with an empty module graph.
    pub fn new() -> Self {
        Self {
            loaded: HashMap::new(),
            exports: HashMap::new(),
            visiting: Vec::new(),
            diagnostics: Vec::new(),
            project_root: None,
            module_root: None,
            source_bytes: 0,
            tokens: 0,
            source_overrides: HashMap::new(),
        }
    }

    /// Replaces one file with an editor buffer without writing it to disk.
    pub fn with_source(mut self, path: &Path, source: &str) -> Self {
        self.source_overrides
            .insert(normalize(path), source.to_owned());
        self
    }

    /// Loads an entry file and recursively expands its imports.
    pub fn load(mut self, entry: &Path) -> Result<Vec<Statement>, OrniError> {
        self.set_project_root(entry)?;
        self.load_module(&normalize(entry), None, true, None)
    }

    /// Loads an entry file and returns non-fatal diagnostics with its program.
    pub fn load_with_diagnostics(
        mut self,
        entry: &Path,
    ) -> Result<(Vec<Statement>, Vec<OrniDiagnostic>), OrniError> {
        self.set_project_root(entry)?;
        let program = self.load_module(&normalize(entry), None, true, None)?;
        Ok((program, self.diagnostics))
    }

    /// Loads one file as a reusable module rather than an executable entry point.
    pub fn load_library(mut self, module: &Path) -> Result<Vec<Statement>, OrniError> {
        self.set_project_root(module)?;
        let namespace = self.module_namespace(&normalize(module));
        self.load_module(&normalize(module), Some(&namespace), false, None)
    }

    /// Loads a reusable module and returns its non-fatal diagnostics.
    pub fn load_library_with_diagnostics(
        mut self,
        module: &Path,
    ) -> Result<(Vec<Statement>, Vec<OrniDiagnostic>), OrniError> {
        self.set_project_root(module)?;
        let namespace = self.module_namespace(&normalize(module));
        let program = self.load_module(&normalize(module), Some(&namespace), false, None)?;
        Ok((program, self.diagnostics))
    }

    fn set_project_root(&mut self, entry: &Path) -> Result<(), OrniError> {
        let parent = entry.parent().unwrap_or_else(|| Path::new("."));
        let project = Project::discover(entry)?;
        let root = project
            .as_ref()
            .map_or(parent, |project| project.root.as_path());
        let source = project
            .as_ref()
            .map_or(parent, |project| project.source.as_path());
        self.module_root = Some(normalize(source));
        self.project_root = Some(fs::canonicalize(root).unwrap_or_else(|_| normalize(root)));
        Ok(())
    }

    fn resolve_import(
        &self,
        importer: &Path,
        segments: &[Token],
        keyword: &Token,
    ) -> Result<PathBuf, OrniError> {
        let build = |base: &Path| {
            let mut candidate = base.to_path_buf();
            for segment in segments {
                candidate.push(identifier_name(segment));
            }
            candidate.set_extension("orni");
            normalize(&candidate)
        };
        let relative = build(importer.parent().unwrap_or_else(|| Path::new(".")));
        let root = build(
            self.module_root
                .as_deref()
                .unwrap_or_else(|| Path::new(".")),
        );
        let relative_exists = relative.is_file() || self.source_overrides.contains_key(&relative);
        let root_exists = root.is_file() || self.source_overrides.contains_key(&root);
        if relative_exists && root_exists && relative != root {
            return Err(OrniError::module(
                keyword.line,
                keyword.column,
                format!(
                    "module import is ambiguous between '{}' and '{}'",
                    relative.display(),
                    root.display()
                ),
            )
            .with_code("ORNI-E122")
            .with_token(keyword));
        }
        Ok(if relative_exists || !root_exists {
            relative
        } else {
            root
        })
    }

    fn module_namespace(&self, path: &Path) -> String {
        let relative = self
            .module_root
            .as_ref()
            .and_then(|root| path.strip_prefix(root).ok())
            .unwrap_or(path);
        let mut without_extension = relative.to_path_buf();
        without_extension.set_extension("");
        without_extension
            .components()
            .filter_map(|component| component.as_os_str().to_str())
            .filter(|component| !component.is_empty())
            .collect::<Vec<_>>()
            .join(".")
    }

    fn load_module(
        &mut self,
        path: &Path,
        namespace: Option<&str>,
        is_entry: bool,
        import_token: Option<&Token>,
    ) -> Result<Vec<Statement>, OrniError> {
        let path = normalize(path);
        let resolved_path = fs::canonicalize(&path).ok();
        if import_token.is_some()
            && self.project_root.as_ref().is_some_and(|root| {
                resolved_path
                    .as_ref()
                    .is_some_and(|canonical_path| !canonical_path.starts_with(root))
            })
        {
            return Err(module_error(
                import_token,
                format!(
                    "module '{}' resolves outside the Orni project root",
                    path.display()
                ),
            )
            .with_code("ORNI-E129"));
        }
        if self.visiting.contains(&path) {
            let chain = self
                .visiting
                .iter()
                .chain(std::iter::once(&path))
                .map(|item| item.display().to_string())
                .collect::<Vec<_>>()
                .join(" -> ");
            return Err(
                module_error(import_token, format!("circular import detected: {chain}"))
                    .with_code("ORNI-E120"),
            );
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
            )
            .with_code("ORNI-E123"));
        }

        if self.loaded.len() + self.visiting.len() >= 256 || self.visiting.len() >= 64 {
            return Err(module_error(
                import_token,
                "module graph exceeds the limit of 256 files or 64 import levels",
            )
            .with_code("ORNI-E141"));
        }
        let override_source = self.source_overrides.get(&path);
        let size = if let Some(source) = override_source {
            source.len() as u64
        } else {
            fs::metadata(&path)
                .map_err(|error| {
                    module_error(import_token, missing_module_message(&path, &error))
                        .with_code("ORNI-E121")
                })?
                .len()
        };
        self.source_bytes = self.source_bytes.saturating_add(size);
        if size > 1_048_576 || self.source_bytes > 8_388_608 {
            return Err(module_error(
                import_token,
                "source exceeds the limit of 1 MiB per file or 8 MiB per module graph",
            )
            .with_code("ORNI-E141"));
        }
        let source = if let Some(source) = override_source {
            source.clone()
        } else {
            fs::read_to_string(&path).map_err(|error| {
                module_error(import_token, missing_module_message(&path, &error))
                    .with_code("ORNI-E121")
            })?
        };
        let tokens = Lexer::new(&source)
            .with_source_name(path.display().to_string())
            .scan_tokens()
            .map_err(|error| error.with_source(&path))?;
        self.tokens += tokens.len();
        if self.tokens > 100_000 {
            return Err(module_error(
                import_token,
                "module graph exceeds the limit of 100000 tokens",
            )
            .with_code("ORNI-E141"));
        }
        let parsed = Parser::new(tokens)
            .parse()
            .map_err(|error| error.with_source(&path))?;

        if !is_entry {
            for statement in &parsed {
                if !matches!(
                    statement,
                    Statement::Import { .. }
                        | Statement::Function { .. }
                        | Statement::Let { .. }
                        | Statement::Contract { .. }
                        | Statement::Provider { .. }
                ) {
                    let (line, column) = statement_token(statement)
                        .map_or((1, 1), |token| (token.line, token.column));
                    return Err(OrniError::module(
                        line,
                        column,
                        "imported modules may only contain imports, contracts, providers, functions, and module variables",
                    ).with_code("ORNI-E126")
                    .with_source(&path));
                }
            }
        }

        let (module_functions, module_variables, module_contracts, module_providers) =
            collect_module_declarations(&parsed, &path)?;
        for statement in &parsed {
            let Statement::Function { name, .. } = statement else {
                continue;
            };
            let function_name = identifier_name(name);
            if function_name.starts_with('_') && !name_is_used(&parsed, function_name) {
                self.diagnostics.push(OrniDiagnostic::warning(
                    name,
                    "ORNI-W003",
                    format!("private function '{function_name}' is never used"),
                ));
            }
        }

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
                    .with_code("ORNI-E116")
                    .with_source(&path));
                }
                if members.is_none()
                    && (module_functions.contains(local_alias)
                        || module_variables.contains(local_alias)
                        || module_contracts.contains(local_alias)
                        || module_providers.contains(local_alias)
                        || selected_imports.contains_key(local_alias))
                {
                    return Err(OrniError::module(
                        keyword.line,
                        keyword.column,
                        format!("module namespace '{local_alias}' conflicts with a declaration"),
                    )
                    .with_code("ORNI-E116")
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
                let imported_path = self.resolve_import(&path, segments, keyword)?;
                let child_namespace = self.module_namespace(&imported_path);
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
                            .with_code("ORNI-E124")
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
                            .with_code("ORNI-E125")
                            .with_token(member));
                        }
                        if module_functions.contains(member_name)
                            || module_variables.contains(member_name)
                            || module_contracts.contains(member_name)
                            || module_providers.contains(member_name)
                            || aliases.contains_key(member_name)
                            || selected_imports.contains_key(member_name)
                        {
                            return Err(OrniError::module(
                                member.line,
                                member.column,
                                format!(
                                    "selectively imported member '{member_name}' conflicts with a declaration"
                                ),
                            ).with_code("ORNI-E116")
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
                module_contracts: &module_contracts,
                module_providers: &module_providers,
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
                    .chain(&module_contracts)
                    .chain(&module_providers)
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
    module_contracts: &'a HashSet<String>,
    module_providers: &'a HashSet<String>,
}

fn transform_statement(
    statement: &mut Statement,
    context: &TransformContext<'_>,
    locals: &mut HashSet<String>,
    module_level: bool,
) -> Result<(), OrniError> {
    match statement {
        Statement::Import { .. } => unreachable!("imports are removed before transformation"),
        Statement::Contract { name, methods } => {
            for method in methods {
                for parameter in &mut method.parameters {
                    transform_type_name(&mut parameter.type_name, context);
                    if let Some(default) = &mut parameter.default_value {
                        transform_expression(default, context, locals)?;
                    }
                }
                transform_type_name(&mut method.return_type, context);
            }
            if module_level && let Some(namespace) = context.namespace {
                qualify_token(name, namespace);
            }
        }
        Statement::Provider {
            name,
            dependencies,
            contract,
            methods,
        } => {
            transform_reference_token(contract, context, context.module_contracts);
            let mut provider_locals = locals.clone();
            for dependency in dependencies {
                transform_type_name(&mut dependency.type_name, context);
                if let Some(default) = &mut dependency.default_value {
                    transform_expression(default, context, &provider_locals)?;
                }
                provider_locals.insert(identifier_name(&dependency.name).to_owned());
            }
            for method in methods {
                transform_statement(method, context, &mut provider_locals, false)?;
            }
            if module_level && let Some(namespace) = context.namespace {
                qualify_token(name, namespace);
            }
        }
        Statement::Bind {
            contract, provider, ..
        } => {
            transform_reference_token(contract, context, context.module_contracts);
            transform_reference_token(provider, context, context.module_providers);
        }
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
            return_type,
            body,
        } => {
            for parameter in &mut *parameters {
                transform_type_name(&mut parameter.type_name, context);
            }
            let mut function_locals = parameters
                .iter()
                .map(|parameter| identifier_name(&parameter.name).to_owned())
                .collect::<HashSet<_>>();
            for parameter in parameters {
                if let Some(default) = &mut parameter.default_value {
                    transform_expression(default, context, &function_locals)?;
                }
            }
            transform_type_name(return_type, context);
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
                        .with_code("ORNI-E124")
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

fn transform_type_name(type_name: &mut TypeName, context: &TransformContext<'_>) {
    let TypeName::Contract(name) = type_name else {
        return;
    };
    *name = resolved_reference(name, context, context.module_contracts);
}

fn transform_reference_token(
    token: &mut Token,
    context: &TransformContext<'_>,
    local_declarations: &HashSet<String>,
) {
    let resolved = resolved_reference(identifier_name(token), context, local_declarations);
    set_identifier(token, resolved);
}

fn resolved_reference(
    name: &str,
    context: &TransformContext<'_>,
    local_declarations: &HashSet<String>,
) -> String {
    if let Some(selected) = context.selected_imports.get(name) {
        return selected.clone();
    }
    if local_declarations.contains(name)
        && let Some(namespace) = context.namespace
    {
        return format!("{namespace}.{name}");
    }
    if let Some((prefix, member)) = name.split_once('.')
        && let Some(namespace) = context.aliases.get(prefix)
    {
        return format!("{namespace}.{member}");
    }
    name.to_owned()
}

fn missing_module_message(path: &Path, error: &std::io::Error) -> String {
    let base = format!("could not read module '{}': {error}", path.display());
    let Some(expected) = path.file_stem().and_then(|name| name.to_str()) else {
        return base;
    };
    let Some(parent) = path.parent() else {
        return base;
    };
    let suggestion = one_edit_names(expected).into_iter().find(|name| {
        let mut candidate = parent.join(name);
        candidate.set_extension("orni");
        candidate.is_file()
    });
    suggestion.map_or(base.clone(), |name| {
        format!("{base}; did you mean '{name}'?")
    })
}

fn one_edit_names(name: &str) -> Vec<String> {
    let characters = name.chars().collect::<Vec<_>>();
    let mut candidates = HashSet::new();
    for index in 0..=characters.len() {
        for replacement in 'a'..='z' {
            let mut inserted = characters.clone();
            inserted.insert(index, replacement);
            candidates.insert(inserted.iter().collect::<String>());
            if index < characters.len() {
                let mut replaced = characters.clone();
                replaced[index] = replacement;
                candidates.insert(replaced.iter().collect::<String>());
            }
        }
        if index < characters.len() {
            let mut removed = characters.clone();
            removed.remove(index);
            candidates.insert(removed.iter().collect::<String>());
        }
        if index + 1 < characters.len() {
            let mut transposed = characters.clone();
            transposed.swap(index, index + 1);
            candidates.insert(transposed.iter().collect::<String>());
        }
    }
    let mut candidates = candidates.into_iter().collect::<Vec<_>>();
    candidates.sort();
    candidates
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
        Statement::Let { name, .. }
        | Statement::Function { name, .. }
        | Statement::Contract { name, .. }
        | Statement::Provider { name, .. } => Some(name),
        Statement::Import { keyword, .. }
        | Statement::Bind { keyword, .. }
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

type ModuleDeclarations = (
    HashSet<String>,
    HashSet<String>,
    HashSet<String>,
    HashSet<String>,
);

fn collect_module_declarations(
    statements: &[Statement],
    source_path: &Path,
) -> Result<ModuleDeclarations, OrniError> {
    let mut functions = HashSet::new();
    let mut variables = HashSet::new();
    let mut contracts = HashSet::new();
    let mut providers = HashSet::new();
    let mut declarations = HashSet::new();

    for statement in statements {
        let (name, token, destination) = match statement {
            Statement::Function { name, .. } => (identifier_name(name), name, &mut functions),
            Statement::Let { name, .. } => (identifier_name(name), name, &mut variables),
            Statement::Contract { name, .. } => (identifier_name(name), name, &mut contracts),
            Statement::Provider { name, .. } => (identifier_name(name), name, &mut providers),
            _ => continue,
        };
        if !declarations.insert(name.to_owned()) {
            return Err(OrniError::module(
                token.line,
                token.column,
                format!("module member '{name}' is already declared"),
            )
            .with_code("ORNI-E116")
            .with_token(token)
            .with_source(source_path));
        }
        destination.insert(name.to_owned());
    }

    Ok((functions, variables, contracts, providers))
}

fn namespace_is_used(statements: &[Statement], namespace: &str) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Import { .. } => false,
        Statement::Contract { methods, .. } => methods.iter().any(|method| {
            method
                .parameters
                .iter()
                .any(|parameter| type_uses_namespace(&parameter.type_name, namespace))
                || type_uses_namespace(&method.return_type, namespace)
        }),
        Statement::Provider {
            dependencies,
            contract,
            methods,
            ..
        } => {
            identifier_name(contract)
                .strip_prefix(namespace)
                .is_some_and(|suffix| suffix.starts_with('.'))
                || dependencies.iter().any(|dependency| {
                    type_uses_namespace(&dependency.type_name, namespace)
                        || dependency
                            .default_value
                            .as_ref()
                            .is_some_and(|value| expression_uses_namespace(value, namespace))
                })
                || namespace_is_used(methods, namespace)
        }
        Statement::Bind {
            contract, provider, ..
        } => [contract, provider].iter().any(|token| {
            identifier_name(token)
                .strip_prefix(namespace)
                .is_some_and(|suffix| suffix.starts_with('.'))
        }),
        Statement::Let { value, .. } => expression_uses_namespace(value, namespace),
        Statement::Function {
            parameters, body, ..
        } => {
            parameters.iter().any(|parameter| {
                type_uses_namespace(&parameter.type_name, namespace)
                    || parameter
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
        Statement::Contract { methods, .. } => methods.iter().any(|method| {
            method
                .parameters
                .iter()
                .any(|parameter| type_uses_name(&parameter.type_name, name))
                || type_uses_name(&method.return_type, name)
        }),
        Statement::Provider {
            dependencies,
            contract,
            methods,
            ..
        } => {
            identifier_name(contract) == name
                || dependencies.iter().any(|dependency| {
                    type_uses_name(&dependency.type_name, name)
                        || dependency
                            .default_value
                            .as_ref()
                            .is_some_and(|value| expression_uses_name(value, name))
                })
                || name_is_used(methods, name)
        }
        Statement::Bind {
            contract, provider, ..
        } => identifier_name(contract) == name || identifier_name(provider) == name,
        Statement::Let { value, .. } => expression_uses_name(value, name),
        Statement::Function {
            parameters, body, ..
        } => {
            parameters.iter().any(|parameter| {
                type_uses_name(&parameter.type_name, name)
                    || parameter
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

fn type_uses_name(type_name: &TypeName, name: &str) -> bool {
    matches!(type_name, TypeName::Contract(contract) if contract == name)
}

fn type_uses_namespace(type_name: &TypeName, namespace: &str) -> bool {
    matches!(type_name, TypeName::Contract(contract) if contract
        .strip_prefix(namespace)
        .is_some_and(|suffix| suffix.starts_with('.')))
}
