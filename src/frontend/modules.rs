use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use crate::{
    Project,
    diagnostics::{OrniDiagnostic, OrniError},
    syntax::{Statement, Token},
};

use super::{Lexer, Parser};

mod analysis;
mod paths;
mod transform;

use analysis::{collect_module_declarations, name_is_used, namespace_is_used};
use paths::{missing_module_message, normalize};
use transform::{TransformContext, identifier_name, transform_statement};

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
            .with_source_name(path.to_string_lossy().replace('\\', "/"))
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
                        | Statement::Model { .. }
                        | Statement::Store { .. }
                        | Statement::Provider { .. }
                        | Statement::Bind { .. }
                        | Statement::Route { .. }
                ) {
                    let (line, column) = statement_token(statement)
                        .map_or((1, 1), |token| (token.line, token.column));
                    return Err(OrniError::module(
                        line,
                        column,
                        "imported modules may only contain imports, contracts, models, stores, providers, bindings, routes, functions, and module variables",
                    ).with_code("ORNI-E126")
                    .with_source(&path));
                }
            }
        }

        let (
            module_functions,
            module_variables,
            module_contracts,
            module_models,
            module_stores,
            module_providers,
        ) = collect_module_declarations(&parsed, &path)?;
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
                        || module_models.contains(local_alias)
                        || module_stores.contains(local_alias)
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
                let imported_path = self.resolve_import(&path, segments, keyword)?;
                let child_namespace = self.module_namespace(&imported_path);
                let imported_program =
                    self.load_module(&imported_path, Some(&child_namespace), false, Some(keyword))?;
                let registers_application_part = imported_program.iter().any(|statement| {
                    matches!(
                        statement,
                        Statement::Model { .. }
                            | Statement::Store { .. }
                            | Statement::Bind { .. }
                            | Statement::Route { .. }
                    )
                });
                if members.is_none()
                    && !registers_application_part
                    && !namespace_is_used(&parsed, local_alias)
                {
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
                            || module_models.contains(member_name)
                            || module_stores.contains(member_name)
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
                module_models: &module_models,
                module_stores: &module_stores,
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
                    .chain(&module_models)
                    .chain(&module_stores)
                    .chain(&module_providers)
                    .cloned()
                    .collect(),
            );
            self.loaded.insert(path, namespace.unwrap_or("").to_owned());
        }
        result
    }
}

fn statement_token(statement: &Statement) -> Option<&Token> {
    match statement {
        Statement::Let { name, .. }
        | Statement::Function { name, .. }
        | Statement::Contract { name, .. }
        | Statement::Model { name, .. }
        | Statement::Store { name, .. }
        | Statement::Provider { name, .. } => Some(name),
        Statement::Import { keyword, .. }
        | Statement::Bind { keyword, .. }
        | Statement::Route { keyword, .. }
        | Statement::Print { keyword, .. }
        | Statement::Return { keyword, .. }
        | Statement::If { keyword, .. }
        | Statement::Assert { keyword, .. }
        | Statement::Test { keyword, .. } => Some(keyword),
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
