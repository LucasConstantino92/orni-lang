/// A lexical token together with its one-based source position.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Token {
    /// The token's semantic category and optional value.
    pub kind: TokenKind,
    /// The one-based source line where the token begins.
    pub line: usize,
    /// The one-based source column where the token begins.
    pub column: usize,
}

impl Token {
    /// Creates a token at the provided source position.
    pub fn new(kind: TokenKind, line: usize, column: usize) -> Self {
        Self { kind, line, column }
    }
}

/// Every token category currently recognized by the Orni lexer.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TokenKind {
    /// The `let` keyword.
    Let,
    /// The `fn` keyword.
    Fn,
    /// The `print` keyword.
    Print,
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
    /// A double-precision numeric literal.
    Number(f64),
    /// A UTF-8 string literal without its surrounding quotes.
    String(String),
    /// The assignment symbol, `=`.
    Equal,
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
    /// A statement terminator, `;`.
    Semicolon,
    /// A synthetic marker placed after the final source character.
    Eof,
}
