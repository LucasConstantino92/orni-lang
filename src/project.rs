//! Project manifests shared by the compiler, command line, and editor.

use std::{
    fs,
    path::{Path, PathBuf},
};

#[cfg(not(target_arch = "wasm32"))]
use crate::HttpLimits;
use crate::{ExecutionLimits, OrniError};

/// Paths and runtime settings declared by the nearest `orni.toml` manifest.
#[derive(Debug, Clone)]
pub struct Project {
    /// Directory containing the manifest.
    pub root: PathBuf,
    /// Directory used to resolve root-relative module imports.
    pub source: PathBuf,
    /// Executable application entry point.
    pub entry: PathBuf,
    /// Resource boundaries applied by the command line runner.
    pub runtime: ExecutionLimits,
    /// Native HTTP boundaries declared by the project manifest.
    #[cfg(not(target_arch = "wasm32"))]
    pub http: HttpLimits,
}

impl Project {
    /// Finds and validates the closest manifest above a file or directory.
    pub fn discover(path: impl AsRef<Path>) -> Result<Option<Self>, OrniError> {
        let path = path.as_ref();
        let start = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(Path::new("."))
        };
        for ancestor in start.ancestors() {
            let root = if ancestor.as_os_str().is_empty() {
                Path::new(".")
            } else {
                ancestor
            };
            let manifest = root.join("orni.toml");
            if manifest.is_file() {
                return Self::read(root, &manifest).map(Some);
            }
        }
        Ok(None)
    }

    fn read(root: &Path, manifest: &Path) -> Result<Self, OrniError> {
        let error = |message: String| {
            OrniError::module(1, 1, message)
                .with_code("ORNI-E130")
                .with_source(manifest)
        };
        if fs::metadata(manifest)
            .map_err(|e| error(e.to_string()))?
            .len()
            > 65_536
        {
            return Err(error("project manifest exceeds 65536 bytes".into()));
        }
        let text = fs::read_to_string(manifest).map_err(|e| error(e.to_string()))?;
        let document = text
            .parse::<toml_edit::DocumentMut>()
            .map_err(|e| error(format!("invalid project manifest: {e}")))?;
        let value = document.as_table();
        for (key, _) in value.iter() {
            if !matches!(key, "project" | "runtime" | "http") {
                return Err(error(format!("unknown manifest section '{key}'")));
            }
        }
        let project = value
            .get("project")
            .and_then(toml_edit::Item::as_table)
            .ok_or_else(|| error("manifest requires a [project] table".into()))?;
        for (key, _) in project.iter() {
            if !matches!(key, "name" | "source" | "entry") {
                return Err(error(format!("unknown project setting '{key}'")));
            }
        }
        if let Some(name) = project.get("name")
            && name.as_str().is_none_or(|name| name.trim().is_empty())
        {
            return Err(error("project.name must be a nonempty string".into()));
        }
        let resolve = |key: &str, fallback: &str| -> Result<PathBuf, OrniError> {
            let text = match project.get(key) {
                Some(value) => value
                    .as_str()
                    .ok_or_else(|| error(format!("project.{key} must be a string")))?,
                None => fallback,
            };
            let path = Path::new(text);
            if path.is_absolute()
                || path.components().any(|part| {
                    matches!(
                        part,
                        std::path::Component::ParentDir | std::path::Component::Prefix(_)
                    )
                })
            {
                return Err(error(format!(
                    "project.{key} must stay inside the project directory"
                )));
            }
            let joined = root.join(path);
            if let (Ok(base), Ok(resolved)) = (fs::canonicalize(root), fs::canonicalize(&joined))
                && !resolved.starts_with(base)
            {
                return Err(error(format!(
                    "project.{key} resolves outside the project directory"
                )));
            }
            Ok(joined)
        };
        let source = resolve("source", ".")?;
        let entry = resolve("entry", "main.orni")?;
        if !source.is_dir() {
            return Err(error(
                "project.source must name an existing directory".into(),
            ));
        }
        if entry
            .extension()
            .is_none_or(|extension| extension != "orni")
        {
            return Err(error("project.entry must name an .orni file".into()));
        }
        let mut runtime = ExecutionLimits::default();
        if let Some(settings) = value.get("runtime") {
            let settings = settings
                .as_table()
                .ok_or_else(|| error("runtime must be a table".into()))?;
            for (key, value) in settings {
                let number = value
                    .as_integer()
                    .and_then(|number| usize::try_from(number).ok())
                    .filter(|number| *number > 0)
                    .ok_or_else(|| {
                        error(format!(
                            "runtime.{key} must be a positive platform-sized integer"
                        ))
                    })?;
                let target = match key {
                    "max_string_bytes" => &mut runtime.max_string_bytes,
                    "max_output_bytes" => &mut runtime.max_output_bytes,
                    "max_retained_string_bytes" => &mut runtime.max_retained_string_bytes,
                    "max_call_depth" => &mut runtime.max_call_depth,
                    "max_variables" => &mut runtime.max_variables,
                    "max_operations" => &mut runtime.max_operations,
                    _ => return Err(error(format!("unknown runtime setting '{key}'"))),
                };
                *target = number;
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        let mut http = HttpLimits::default();
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(settings) = value.get("http") {
            let settings = settings
                .as_table()
                .ok_or_else(|| error("http must be a table".into()))?;
            for (key, value) in settings {
                let number = value
                    .as_integer()
                    .and_then(|number| usize::try_from(number).ok())
                    .filter(|number| *number > 0)
                    .ok_or_else(|| error(format!("http.{key} must be a positive integer")))?;
                match key {
                    "connection_timeout_seconds" => http.connection_timeout_seconds = number as u64,
                    "max_connections" => http.max_connections = number,
                    "max_connections_per_ip" => http.max_connections_per_ip = number,
                    "max_requests_per_connection" => http.max_requests_per_connection = number,
                    "max_requests_per_ip" => http.max_requests_per_ip = number,
                    "request_window_seconds" => http.request_window_seconds = number as u64,
                    "cache_ttl_seconds" => http.cache_ttl_seconds = number as u64,
                    "max_cache_entries" => http.max_cache_entries = number,
                    "max_request_bytes" => http.max_request_bytes = number,
                    "max_cache_bytes" => http.max_cache_bytes = number,
                    _ => return Err(error(format!("unknown http setting '{key}'"))),
                }
            }
            if http.max_connections_per_ip > http.max_connections {
                return Err(error(
                    "http.max_connections_per_ip cannot exceed http.max_connections".into(),
                ));
            }
        }
        Ok(Self {
            root: root.to_path_buf(),
            source,
            entry,
            runtime,
            #[cfg(not(target_arch = "wasm32"))]
            http,
        })
    }
}
