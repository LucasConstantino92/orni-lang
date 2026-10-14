# Diagnostic code catalog

Specific error identities are attached explicitly by the compiler at error creation
sites. `ORNI-E` codes describe the correction category, independently of the phase
(lexical, syntax, semantic, module, or runtime). Changing English wording must not
change an identity. Do not reuse an existing code for a different correction.

The editor selects guidance by code. Message parsing is retained only for older
compilers and phase-wide fallback diagnostics. Automatic edits may additionally
inspect details and source declarations, and must be omitted when unsafe.

| Code | Correction category |
|---|---|
| `ORNI-E101` | Break the provider dependency cycle |
| `ORNI-E102` | Configure the missing provider binding |
| `ORNI-E103` | Declare or import the missing name |
| `ORNI-E104` | Keep the value inside the integer range |
| `ORNI-E105` | Use a signed type before negation |
| `ORNI-E106` | Adjust the string capacity |
| `ORNI-E107` | Make the value match the declared type |
| `ORNI-E108` | Adjust the function arguments |
| `ORNI-E109` | Implement the missing contract method |
| `ORNI-E110` | Match the contract method signature |
| `ORNI-E111` | Remove the unsupported default value |
| `ORNI-E112` | Declare an injectable contract dependency |
| `ORNI-E113` | Declare or import the contract or provider |
| `ORNI-E114` | Bind a compatible provider |
| `ORNI-E115` | Keep one binding per contract |
| `ORNI-E116` | Remove or rename the conflicting declaration |
| `ORNI-E117` | Return a compatible value on every path |
| `ORNI-E118` | Move return into a function |
| `ORNI-E119` | Use a method declared by the contract |
| `ORNI-E120` | Break the circular module imports |
| `ORNI-E121` | Correct the module file or import path |
| `ORNI-E122` | Make the module import unambiguous |
| `ORNI-E123` | Use one canonical module import |
| `ORNI-E124` | Use the module's public API |
| `ORNI-E125` | Import an existing public declaration |
| `ORNI-E126` | Move application setup into the entry file |
| `ORNI-E127` | Check the program as a file |
| `ORNI-E128` | Choose a selective import or a namespace alias |
| `ORNI-E129` | Keep source paths inside the project |
| `ORNI-E130` | Correct the orni.toml project settings |
| `ORNI-E131` | Guard the divisor before dividing |
| `ORNI-E132` | Prevent integer arithmetic overflow |
| `ORNI-E133` | Keep large exact values as integers |
| `ORNI-E134` | Add a terminating recursion condition |
| `ORNI-E135` | Reduce retained string data |
| `ORNI-E136` | Reduce captured output |
| `ORNI-E137` | Bound string growth before allocation |
| `ORNI-E138` | Reduce simultaneously active bindings |
| `ORNI-E139` | Reduce work or terminate the computation |
| `ORNI-E140` | Simplify the inference workload |
| `ORNI-E141` | Reduce the compilation input |
| `ORNI-E142` | Flatten the expression or nested blocks |
| `ORNI-E143` | Close the string literal |
| `ORNI-E144` | Use exactly one character |
| `ORNI-E145` | Use a supported escape sequence |
| `ORNI-E146` | Remove or replace the unexpected character |
| `ORNI-E147` | Place required parameters first |
| `ORNI-E148` | Use a positive string byte capacity |
| `ORNI-E149` | Remove pub from the declaration |
| `ORNI-E150` | Call a named function or provider method |
| `ORNI-E151` | Use a supported member-access target |

Phase-wide fallback codes (`ORNI-L001`, `ORNI-S001`, `ORNI-T001`, `ORNI-M001`,
`ORNI-R001`) remain supported. Warning codes `ORNI-W001` through `ORNI-W004`
keep their existing meanings. The error's phase, severity, source span, and
human-readable message remain available separately through the Rust API.

When adding an error category, add the compiler identity, editor guidance, and
a regression test. The editor tests check that every emitted specific identity
has guidance and that changing the message does not change its explanation.
