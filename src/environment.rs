use std::collections::HashMap;

use crate::ast::Value;

/// Stores variables visible to the interpreter.
///
/// The first map is the global scope. Each function call adds a new local
/// scope and records its boundary. Looking up a name searches the active
/// function first and then the global scope, preventing a called function
/// from accidentally reading its caller's local variables.
pub(crate) struct Environment {
    scopes: Vec<HashMap<String, Value>>,
    function_boundaries: Vec<usize>,
}

impl Environment {
    /// Creates an environment containing an empty global scope.
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            function_boundaries: Vec::new(),
        }
    }

    /// Starts an isolated local scope for a function call.
    pub fn enter_function_scope(&mut self) {
        self.scopes.push(HashMap::new());
        self.function_boundaries.push(self.scopes.len() - 1);
    }

    /// Removes the current function scope and all values declared inside it.
    pub fn exit_function_scope(&mut self) {
        let boundary = self
            .function_boundaries
            .pop()
            .expect("a function scope must exist before it can be removed");
        self.scopes.truncate(boundary);
    }

    /// Defines or replaces a variable in the innermost active scope.
    pub fn define(&mut self, name: String, value: Value) {
        self.scopes
            .last_mut()
            .expect("an environment always has a scope")
            .insert(name, value);
    }

    /// Finds a variable in the active function scope or the global scope.
    pub fn get(&self, name: &str) -> Option<&Value> {
        let local = self.function_boundaries.last().and_then(|boundary| {
            self.scopes[*boundary..]
                .iter()
                .rev()
                .find_map(|scope| scope.get(name))
        });

        local.or_else(|| self.scopes[0].get(name))
    }
}
