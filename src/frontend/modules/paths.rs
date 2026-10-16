//! Safe module path normalization and missing-file diagnostics.

use std::{
    collections::HashSet,
    path::{Component, Path, PathBuf},
};

pub(super) fn missing_module_message(path: &Path, error: &std::io::Error) -> String {
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

pub(super) fn normalize(path: &Path) -> PathBuf {
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
