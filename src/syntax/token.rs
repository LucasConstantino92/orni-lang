use std::sync::Arc;

/// A lexical token together with its one-based source position.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Token {
    /// The token's semantic category and optional value.
    pub kind: TokenKind,
    /// The one-based source line where the token begins.
    pub line: usize,
    /// The one-based source column where the token begins.
    pub column: usize,
    /// The one-based source line where the token ends.
    pub end_line: usize,
    /// The one-based exclusive source column where the token ends.
    pub end_column: usize,
    /// The file that produced this token, when it came from a module graph.
    pub source: Option<Arc<str>>,
}

impl Token {
    /// Creates a token at the provided source position.
    pub fn new(kind: TokenKind, line: usize, column: usize) -> Self {
        Self {
            kind,
            line,
            column,
            end_line: line,
            end_column: column.saturating_add(1),
            source: None,
        }
    }

    /// Creates a token tied to a source file shared by the complete token stream.
    pub fn in_source(
        kind: TokenKind,
        line: usize,
        column: usize,
        end_line: usize,
        end_column: usize,
        source: Arc<str>,
    ) -> Self {
        Self {
            kind,
            line,
            column,
            end_line,
            end_column,
            source: Some(source),
        }
    }
}

/// Every token category currently recognized by the Orni lexer.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TokenKind {
    /// The `let` keyword.
    Let,
    /// The `func` keyword.
    Func,
    /// The `pub` keyword.
    Pub,
    /// The `import` keyword.
    Import,
    /// The `contract` keyword.
    Contract,
    /// The `model` keyword for typed data schemas.
    Model,
    /// The `store` keyword for a persistent in-memory model collection.
    Store,
    /// The `provider` keyword.
    Provider,
    /// The `for` keyword connecting a provider to a contract.
    For,
    /// The `bind` keyword.
    Bind,
    /// The `route` keyword for an HTTP route declaration.
    Route,
    /// The `get` HTTP method keyword.
    Get,
    /// The `post` HTTP method keyword.
    Post,
    /// The `put` HTTP method keyword.
    Put,
    /// The `patch` HTTP method keyword.
    Patch,
    /// The `delete` HTTP method keyword.
    Delete,
    /// The `to` keyword connecting a binding to its provider.
    To,
    /// The `as` keyword used by import aliases.
    As,
    /// The `print` keyword.
    Print,
    /// The `assert` keyword used by test files.
    Assert,
    /// The `return` keyword.
    Return,
    /// The `if` keyword.
    If,
    /// The `else` keyword.
    Else,
    /// The `true` boolean literal.
    True,
    /// The `false` boolean literal.
    False,
    /// A user-defined name.
    Identifier(String),
    /// An integer literal stored losslessly until its target type is known.
    Int(i128),
    /// A double-precision floating-point literal.
    Double(f64),
    /// A UTF-8 string literal without its surrounding quotes.
    String(String),
    /// A character literal without its surrounding quotes.
    Char(char),
    /// The assignment symbol, `=`.
    Equal,
    /// The route declaration arrow, `=>`.
    FatArrow,
    /// The equality comparison symbol, `==`.
    EqualEqual,
    /// The logical negation symbol, `!`.
    Bang,
    /// The inequality comparison symbol, `!=`.
    BangEqual,
    /// The greater-than comparison symbol, `>`.
    Greater,
    /// The greater-than-or-equal comparison symbol, `>=`.
    GreaterEqual,
    /// The less-than comparison symbol, `<`.
    Less,
    /// The less-than-or-equal comparison symbol, `<=`.
    LessEqual,
    /// The addition or string-concatenation symbol, `+`.
    Plus,
    /// The subtraction or numeric-negation symbol, `-`.
    Minus,
    /// The function return type arrow, `->`.
    Arrow,
    /// The multiplication symbol, `*`.
    Star,
    /// The division symbol, `/`.
    Slash,
    /// An opening parenthesis, `(`.
    LeftParen,
    /// A closing parenthesis, `)`.
    RightParen,
    /// An opening brace, `{`.
    LeftBrace,
    /// A closing brace, `}`.
    RightBrace,
    /// An argument or parameter separator, `,`.
    Comma,
    /// A module path separator, `.`.
    Dot,
    /// A type annotation separator, `:`.
    Colon,
    /// A statement terminator, `;`.
    Semicolon,
    /// A synthetic marker placed after the final source character.
    Eof,
}
