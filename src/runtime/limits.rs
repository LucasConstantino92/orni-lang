/// Resource boundaries applied while an Orni program is running.
///
/// Rust ownership releases runtime values when their lexical scope ends. These
/// limits complement that automatic cleanup by stopping programs that attempt
/// to retain or produce an unreasonable amount of data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionLimits {
    /// Maximum UTF-8 byte length of one generated string.
    pub max_string_bytes: usize,
    /// Maximum UTF-8 byte length captured from `print` statements.
    pub max_output_bytes: usize,
    /// Maximum combined string bytes retained by active variables.
    pub max_retained_string_bytes: usize,
    /// Maximum number of simultaneously active function calls.
    pub max_call_depth: usize,
    /// Maximum number of simultaneously retained variable bindings.
    pub max_variables: usize,
    /// Maximum number of statements and expressions evaluated.
    pub max_operations: usize,
}

impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            max_string_bytes: 1_048_576,
            max_output_bytes: 1_048_576,
            max_retained_string_bytes: 8_388_608,
            max_call_depth: 128,
            max_variables: 10_000,
            max_operations: 100_000,
        }
    }
}
