use std::collections::HashMap;

use crate::syntax::Value;

/// Stores variables visible to the interpreter.
///
/// The first map is the global scope. Each function call adds a new local
/// scope and records its boundary. Looking up a name searches the active
/// function first and then the global scope, preventing a called function
/// from accidentally reading its caller's local variables.
pub(crate) struct Environment {
    scopes: Vec<HashMap<String, Value>>,
    function_boundaries: Vec<usize>,
    bindings: usize,
    string_bytes: usize,
}

impl Environment {
    /// Creates an environment containing an empty global scope.
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            function_boundaries: Vec::new(),
            bindings: 0,
            string_bytes: 0,
        }
    }

    /// Starts an isolated local scope for a function call.
    pub fn enter_function_scope(&mut self) {
        self.scopes.push(HashMap::new());
        self.function_boundaries.push(self.scopes.len() - 1);
    }

    /// Starts a nested block scope inside the current execution context.
    pub fn enter_block_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    /// Removes the innermost block scope.
    pub fn exit_block_scope(&mut self) {
        let minimum_length = self
            .function_boundaries
            .last()
            .map_or(1, |boundary| boundary + 1);
        debug_assert!(
            self.scopes.len() > minimum_length,
            "a block scope must exist before it can be removed"
        );
        if let Some(scope) = self.scopes.pop() {
            self.release_scope(&scope);
        }
    }

    /// Removes the current function scope and all values declared inside it.
    pub fn exit_function_scope(&mut self) {
        let boundary = self
            .function_boundaries
            .pop()
            .expect("a function scope must exist before it can be removed");
        while self.scopes.len() > boundary {
            if let Some(scope) = self.scopes.pop() {
                self.release_scope(&scope);
            }
        }
    }

    /// Defines or replaces a variable in the innermost active scope.
    pub fn define(&mut self, name: String, value: Value) {
        let added_bytes = string_bytes(&value);
        let previous = self
            .scopes
            .last_mut()
            .expect("an environment always has a scope")
            .insert(name, value);
        self.string_bytes += added_bytes;
        if let Some(previous) = previous {
            self.string_bytes -= string_bytes(&previous);
        } else {
            self.bindings += 1;
        }
    }

    /// Finds a binding in the innermost scope only.
    pub fn current_value(&self, name: &str) -> Option<&Value> {
        self.scopes.last().and_then(|scope| scope.get(name))
    }

    /// Counts bindings that are currently retained by active scopes.
    pub fn binding_count(&self) -> usize {
        self.bindings
    }

    /// Counts string bytes conservatively across all active bindings.
    pub fn retained_string_bytes(&self) -> usize {
        self.string_bytes
    }

    fn release_scope(&mut self, scope: &HashMap<String, Value>) {
        self.bindings -= scope.len();
        self.string_bytes -= scope.values().map(string_bytes).sum::<usize>();
    }

    /// Finds a variable in the active function scope or the global scope.
    pub fn get(&self, name: &str) -> Option<&Value> {
        let boundary = self.function_boundaries.last().copied().unwrap_or(0);
        let visible = self.scopes[boundary..]
            .iter()
            .rev()
            .find_map(|scope| scope.get(name));

        if boundary == 0 {
            visible
        } else {
            visible.or_else(|| self.scopes[0].get(name))
        }
    }
}

fn string_bytes(value: &Value) -> usize {
    match value {
        Value::String(value) => value.len(),
        _ => 0,
    }
}
