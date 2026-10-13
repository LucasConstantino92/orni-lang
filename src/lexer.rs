use crate::{
    error::OrniError,
    token::{Token, TokenKind},
};

/// Converts Orni source characters into a stream of [`Token`] values.
pub(crate) struct Lexer {
    chars: Vec<char>,
    current: usize,
    line: usize,
    column: usize,
}

impl Lexer {
    /// Creates a lexer positioned at the first character of `source`.
    ///
    /// Characters are owned by the lexer so scanning does not need to retain a
    /// borrow of the original source string.
    pub fn new(source: &str) -> Self {
        Self {
            chars: source.chars().collect(),
            current: 0,
            line: 1,
            column: 1,
        }
    }

    /// Scans the entire source and appends a final [`TokenKind::Eof`] marker.
    pub fn scan_tokens(mut self) -> Result<Vec<Token>, OrniError> {
        let mut tokens = Vec::new();

        while !self.is_at_end() {
            let line = self.line;
            let column = self.column;
            let character = self.advance();

            let kind = match character {
                ' ' | '\r' | '\t' => continue,
                '\n' => continue,
                '(' => TokenKind::LeftParen,
                ')' => TokenKind::RightParen,
                '{' => TokenKind::LeftBrace,
                '}' => TokenKind::RightBrace,
                ',' => TokenKind::Comma,
                ';' => TokenKind::Semicolon,
                '=' => TokenKind::Equal,
                '+' => TokenKind::Plus,
                '-' => TokenKind::Minus,
                '*' => TokenKind::Star,
                '/' if self.peek() == Some('/') => {
                    while self.peek().is_some_and(|next| next != '\n') {
                        self.advance();
                    }
                    continue;
                }
                '/' => TokenKind::Slash,
                '"' => self.string(line, column)?,
                digit if digit.is_ascii_digit() => self.number(digit),
                first if is_identifier_start(first) => self.identifier(first),
                other => {
                    return Err(OrniError::lexical(
                        line,
                        column,
                        format!("unexpected character '{other}'"),
                    ));
                }
            };

            tokens.push(Token::new(kind, line, column));
        }

        tokens.push(Token::new(TokenKind::Eof, self.line, self.column));
        Ok(tokens)
    }

    /// Scans a string after its opening quote has been consumed.
    fn string(&mut self, line: usize, column: usize) -> Result<TokenKind, OrniError> {
        let mut value = String::new();

        while let Some(character) = self.peek() {
            if character == '"' {
                self.advance();
                return Ok(TokenKind::String(value));
            }

            let character = self.advance();
            if character == '\\' {
                let Some(escaped) = self.peek() else {
                    break;
                };
                self.advance();
                value.push(match escaped {
                    'n' => '\n',
                    't' => '\t',
                    '"' => '"',
                    '\\' => '\\',
                    other => other,
                });
            } else {
                value.push(character);
            }
        }

        Err(OrniError::lexical(line, column, "unterminated string"))
    }

    /// Scans a numeric literal after its first digit has been consumed.
    fn number(&mut self, first: char) -> TokenKind {
        let mut value = String::from(first);

        while self.peek().is_some_and(|next| next.is_ascii_digit()) {
            value.push(self.advance());
        }

        if self.peek() == Some('.') && self.peek_next().is_some_and(|next| next.is_ascii_digit()) {
            value.push(self.advance());
            while self.peek().is_some_and(|next| next.is_ascii_digit()) {
                value.push(self.advance());
            }
        }

        TokenKind::Number(value.parse().expect("number validated by the lexer"))
    }

    /// Scans an identifier and promotes reserved words to keyword tokens.
    fn identifier(&mut self, first: char) -> TokenKind {
        let mut value = String::from(first);
        while self.peek().is_some_and(is_identifier_continue) {
            value.push(self.advance());
        }

        match value.as_str() {
            "let" => TokenKind::Let,
            "fn" => TokenKind::Fn,
            "print" => TokenKind::Print,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            _ => TokenKind::Identifier(value),
        }
    }

    /// Consumes one character and updates the source position.
    fn advance(&mut self) -> char {
        let character = self.chars[self.current];
        self.current += 1;
        if character == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        character
    }

    /// Returns the current character without consuming it.
    fn peek(&self) -> Option<char> {
        self.chars.get(self.current).copied()
    }

    /// Returns the character after the current one without consuming it.
    fn peek_next(&self) -> Option<char> {
        self.chars.get(self.current + 1).copied()
    }

    /// Reports whether every source character has been consumed.
    fn is_at_end(&self) -> bool {
        self.current >= self.chars.len()
    }
}

/// Reports whether a character may begin an Orni identifier.
fn is_identifier_start(character: char) -> bool {
    character == '_' || character.is_alphabetic()
}

/// Reports whether a character may continue an Orni identifier.
fn is_identifier_continue(character: char) -> bool {
    character == '_' || character.is_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::Lexer;
    use crate::token::TokenKind;

    #[test]
    fn recognizes_keywords_literals_and_comments() {
        let tokens = Lexer::new("let name = \"Orni\"; // comment\nprint name;")
            .scan_tokens()
            .expect("the input should be valid");
        let kinds: Vec<_> = tokens.into_iter().map(|token| token.kind).collect();

        assert_eq!(
            kinds,
            vec![
                TokenKind::Let,
                TokenKind::Identifier("name".into()),
                TokenKind::Equal,
                TokenKind::String("Orni".into()),
                TokenKind::Semicolon,
                TokenKind::Print,
                TokenKind::Identifier("name".into()),
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }
}
