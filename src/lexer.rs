use thiserror::Error;

#[derive(Debug, Error)]
pub enum LexerError {
    #[error("Unexpected character '{0}' at position {1}")]
    UnexpectedChar(char, usize),
    #[error("Unterminated string at position {0}")]
    UnterminatedString(usize),
    #[error("Invalid number at position {0}")]
    InvalidNumber(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Ident(String),
    IntLiteral(i32),
    FloatLiteral(f32),
    StringLiteral(String),
    CharLiteral(char),
    BoolLiteral(bool),

    As,
    Struct,
    Enum,
    Trait,
    Impl,
    Fn,
    Let,
    Const,
    Mut,
    If,
    Else,
    While,
    For,
    In,
    Match,
    Return,
    Self_,
    Use,
    Mod,
    Pub,
    Super,
    Crate,

    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
    Semicolon,
    Comma,
    Dot,
    DotDot,
    Colon,
    ColonColon,
    Arrow,
    FatArrow,

    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Ampersand,
    Pipe,
    Caret,
    Tilde,
    Bang,

    Eq,
    EqEq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Shl,
    Shr,

    AndAnd,
    OrOr,

    Underscore,

    Eof,
}

pub struct Lexer {
    input: Vec<char>,
    position: usize,
    current: Option<char>,
}

impl Lexer {
    fn new(input: &str) -> Self {
        let chars: Vec<char> = input.chars().collect();
        let current = chars.first().copied();
        Self {
            input: chars,
            position: 0,
            current,
        }
    }

    fn advance(&mut self) {
        self.position += 1;
        self.current = self.input.get(self.position).copied();
    }

    fn peek(&self) -> Option<char> {
        self.input.get(self.position + 1).copied()
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.current {
            if ch.is_whitespace() {
                self.advance();
            } else if ch == '/' && self.peek() == Some('/') {
                self.advance();
                self.advance();
                while self.current.is_some() && self.current != Some('\n') {
                    self.advance();
                }
            } else {
                break;
            }
        }
    }

    fn read_ident(&mut self) -> String {
        let mut ident = String::new();
        while let Some(ch) = self.current {
            if ch.is_alphanumeric() || ch == '_' {
                ident.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        ident
    }

    fn read_number(&mut self) -> Result<Token, LexerError> {
        let start_pos = self.position;

        // Check for hexadecimal prefix
        if self.current == Some('0') && self.peek() == Some('x') {
            self.advance(); // consume '0'
            self.advance(); // consume 'x'

            let mut hex_str = String::new();
            while let Some(ch) = self.current {
                if ch.is_ascii_hexdigit() {
                    hex_str.push(ch);
                    self.advance();
                } else {
                    break;
                }
            }

            if hex_str.is_empty() {
                return Err(LexerError::InvalidNumber(start_pos));
            }

            return i32::from_str_radix(&hex_str, 16)
                .map(Token::IntLiteral)
                .map_err(|_| LexerError::InvalidNumber(start_pos));
        }

        // Regular decimal number
        let mut num_str = String::new();
        let mut is_float = false;

        while let Some(ch) = self.current {
            if ch.is_ascii_digit() {
                num_str.push(ch);
                self.advance();
            } else if ch == '.' && !is_float && self.peek().map_or(false, |c| c.is_ascii_digit()) {
                is_float = true;
                num_str.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        if is_float {
            num_str
                .parse::<f32>()
                .map(Token::FloatLiteral)
                .map_err(|_| LexerError::InvalidNumber(start_pos))
        } else {
            num_str
                .parse::<i32>()
                .map(Token::IntLiteral)
                .map_err(|_| LexerError::InvalidNumber(start_pos))
        }
    }

    fn read_string(&mut self) -> Result<String, LexerError> {
        let start_pos = self.position;
        self.advance();
        let mut string = String::new();

        while let Some(ch) = self.current {
            if ch == '"' {
                self.advance();
                return Ok(string);
            } else if ch == '\\' {
                self.advance();
                match self.current {
                    Some('n') => string.push('\n'),
                    Some('t') => string.push('\t'),
                    Some('r') => string.push('\r'),
                    Some('\\') => string.push('\\'),
                    Some('"') => string.push('"'),
                    _ => {}
                }
                self.advance();
            } else {
                string.push(ch);
                self.advance();
            }
        }

        Err(LexerError::UnterminatedString(start_pos))
    }

    fn read_char(&mut self) -> Result<char, LexerError> {
        let start_pos = self.position;
        self.advance();

        let ch = match self.current {
            Some('\\') => {
                self.advance();
                match self.current {
                    Some('n') => '\n',
                    Some('t') => '\t',
                    Some('r') => '\r',
                    Some('\\') => '\\',
                    Some('\'') => '\'',
                    Some(c) => c,
                    None => return Err(LexerError::UnexpectedChar('\0', start_pos)),
                }
            }
            Some(c) => c,
            None => return Err(LexerError::UnexpectedChar('\0', start_pos)),
        };

        self.advance();
        if self.current != Some('\'') {
            return Err(LexerError::UnexpectedChar(
                self.current.unwrap_or('\0'),
                self.position,
            ));
        }
        self.advance();

        Ok(ch)
    }

    fn next_token(&mut self) -> Result<Token, LexerError> {
        self.skip_whitespace();

        match self.current {
            None => Ok(Token::Eof),
            Some(ch) => {
                if ch.is_alphabetic() || ch == '_' {
                    let ident = self.read_ident();
                    Ok(match ident.as_str() {
                        "as" => Token::As,
                        "struct" => Token::Struct,
                        "enum" => Token::Enum,
                        "trait" => Token::Trait,
                        "impl" => Token::Impl,
                        "fn" => Token::Fn,
                        "let" => Token::Let,
                        "const" => Token::Const,
                        "mut" => Token::Mut,
                        "if" => Token::If,
                        "else" => Token::Else,
                        "while" => Token::While,
                        "for" => Token::For,
                        "in" => Token::In,
                        "match" => Token::Match,
                        "return" => Token::Return,
                        "self" => Token::Self_,
                        "use" => Token::Use,
                        "mod" => Token::Mod,
                        "pub" => Token::Pub,
                        "super" => Token::Super,
                        "crate" => Token::Crate,
                        "true" => Token::BoolLiteral(true),
                        "false" => Token::BoolLiteral(false),
                        "_" => Token::Underscore,
                        _ => Token::Ident(ident),
                    })
                } else if ch.is_ascii_digit() {
                    self.read_number()
                } else {
                    let pos = self.position;
                    match ch {
                        '"' => {
                            let string = self.read_string()?;
                            Ok(Token::StringLiteral(string))
                        }
                        '\'' => {
                            let char_lit = self.read_char()?;
                            Ok(Token::CharLiteral(char_lit))
                        }
                        '(' => {
                            self.advance();
                            Ok(Token::LeftParen)
                        }
                        ')' => {
                            self.advance();
                            Ok(Token::RightParen)
                        }
                        '{' => {
                            self.advance();
                            Ok(Token::LeftBrace)
                        }
                        '}' => {
                            self.advance();
                            Ok(Token::RightBrace)
                        }
                        '[' => {
                            self.advance();
                            Ok(Token::LeftBracket)
                        }
                        ']' => {
                            self.advance();
                            Ok(Token::RightBracket)
                        }
                        ';' => {
                            self.advance();
                            Ok(Token::Semicolon)
                        }
                        ',' => {
                            self.advance();
                            Ok(Token::Comma)
                        }
                        '.' => {
                            self.advance();
                            if self.current == Some('.') {
                                self.advance();
                                Ok(Token::DotDot)
                            } else {
                                Ok(Token::Dot)
                            }
                        }
                        ':' => {
                            self.advance();
                            if self.current == Some(':') {
                                self.advance();
                                Ok(Token::ColonColon)
                            } else {
                                Ok(Token::Colon)
                            }
                        }
                        '+' => {
                            self.advance();
                            Ok(Token::Plus)
                        }
                        '-' => {
                            self.advance();
                            if self.current == Some('>') {
                                self.advance();
                                Ok(Token::Arrow)
                            } else {
                                Ok(Token::Minus)
                            }
                        }
                        '*' => {
                            self.advance();
                            Ok(Token::Star)
                        }
                        '/' => {
                            self.advance();
                            Ok(Token::Slash)
                        }
                        '%' => {
                            self.advance();
                            Ok(Token::Percent)
                        }
                        '&' => {
                            self.advance();
                            if self.current == Some('&') {
                                self.advance();
                                Ok(Token::AndAnd)
                            } else {
                                Ok(Token::Ampersand)
                            }
                        }
                        '|' => {
                            self.advance();
                            if self.current == Some('|') {
                                self.advance();
                                Ok(Token::OrOr)
                            } else {
                                Ok(Token::Pipe)
                            }
                        }
                        '^' => {
                            self.advance();
                            Ok(Token::Caret)
                        }
                        '~' => {
                            self.advance();
                            Ok(Token::Tilde)
                        }
                        '!' => {
                            self.advance();
                            if self.current == Some('=') {
                                self.advance();
                                Ok(Token::Ne)
                            } else {
                                Ok(Token::Bang)
                            }
                        }
                        '=' => {
                            self.advance();
                            if self.current == Some('=') {
                                self.advance();
                                Ok(Token::EqEq)
                            } else if self.current == Some('>') {
                                self.advance();
                                Ok(Token::FatArrow)
                            } else {
                                Ok(Token::Eq)
                            }
                        }
                        '<' => {
                            self.advance();
                            if self.current == Some('=') {
                                self.advance();
                                Ok(Token::Le)
                            } else if self.current == Some('<') {
                                self.advance();
                                Ok(Token::Shl)
                            } else {
                                Ok(Token::Lt)
                            }
                        }
                        '>' => {
                            self.advance();
                            if self.current == Some('=') {
                                self.advance();
                                Ok(Token::Ge)
                            } else if self.current == Some('>') {
                                self.advance();
                                Ok(Token::Shr)
                            } else {
                                Ok(Token::Gt)
                            }
                        }
                        _ => Err(LexerError::UnexpectedChar(ch, pos)),
                    }
                }
            }
        }
    }
}

pub fn tokenize(input: &str) -> Result<Vec<Token>, LexerError> {
    let mut lexer = Lexer::new(input);
    let mut tokens = Vec::new();

    loop {
        let token = lexer.next_token()?;
        if token == Token::Eof {
            tokens.push(token);
            break;
        }
        tokens.push(token);
    }

    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keywords() {
        let input =
            "struct enum trait impl fn let const mut if else while for in match return self use";
        let tokens = tokenize(input).unwrap();

        assert_eq!(tokens[0], Token::Struct);
        assert_eq!(tokens[1], Token::Enum);
        assert_eq!(tokens[2], Token::Trait);
        assert_eq!(tokens[3], Token::Impl);
        assert_eq!(tokens[4], Token::Fn);
        assert_eq!(tokens[5], Token::Let);
        assert_eq!(tokens[6], Token::Const);
        assert_eq!(tokens[7], Token::Mut);
        assert_eq!(tokens[8], Token::If);
        assert_eq!(tokens[9], Token::Else);
        assert_eq!(tokens[10], Token::While);
        assert_eq!(tokens[11], Token::For);
        assert_eq!(tokens[12], Token::In);
        assert_eq!(tokens[13], Token::Match);
        assert_eq!(tokens[14], Token::Return);
        assert_eq!(tokens[15], Token::Self_);
        assert_eq!(tokens[16], Token::Use);
    }

    #[test]
    fn test_identifiers() {
        let input = "foo bar_baz _test test123";
        let tokens = tokenize(input).unwrap();

        assert_eq!(tokens[0], Token::Ident("foo".to_string()));
        assert_eq!(tokens[1], Token::Ident("bar_baz".to_string()));
        assert_eq!(tokens[2], Token::Ident("_test".to_string()));
        assert_eq!(tokens[3], Token::Ident("test123".to_string()));
    }

    #[test]
    fn test_numbers() {
        let input = "42 3.14 0 999";
        let tokens = tokenize(input).unwrap();

        assert_eq!(tokens[0], Token::IntLiteral(42));
        assert_eq!(tokens[1], Token::FloatLiteral(3.14));
        assert_eq!(tokens[2], Token::IntLiteral(0));
        assert_eq!(tokens[3], Token::IntLiteral(999));
    }

    #[test]
    fn test_strings() {
        let input = r#""hello" "world\n" "escaped\"quote""#;
        let tokens = tokenize(input).unwrap();

        assert_eq!(tokens[0], Token::StringLiteral("hello".to_string()));
        assert_eq!(tokens[1], Token::StringLiteral("world\n".to_string()));
        assert_eq!(
            tokens[2],
            Token::StringLiteral("escaped\"quote".to_string())
        );
    }

    #[test]
    fn test_chars() {
        let input = "'a' '\\n' '\\''";
        let tokens = tokenize(input).unwrap();

        assert_eq!(tokens[0], Token::CharLiteral('a'));
        assert_eq!(tokens[1], Token::CharLiteral('\n'));
        assert_eq!(tokens[2], Token::CharLiteral('\''));
    }

    #[test]
    fn test_operators() {
        let input = "+ - * / % & | ^ ~ ! && || == != < <= > >= << >> = -> =>";
        let tokens = tokenize(input).unwrap();

        assert_eq!(tokens[0], Token::Plus);
        assert_eq!(tokens[1], Token::Minus);
        assert_eq!(tokens[2], Token::Star);
        assert_eq!(tokens[3], Token::Slash);
        assert_eq!(tokens[4], Token::Percent);
        assert_eq!(tokens[5], Token::Ampersand);
        assert_eq!(tokens[6], Token::Pipe);
        assert_eq!(tokens[7], Token::Caret);
        assert_eq!(tokens[8], Token::Tilde);
        assert_eq!(tokens[9], Token::Bang);
        assert_eq!(tokens[10], Token::AndAnd);
        assert_eq!(tokens[11], Token::OrOr);
        assert_eq!(tokens[12], Token::EqEq);
        assert_eq!(tokens[13], Token::Ne);
        assert_eq!(tokens[14], Token::Lt);
        assert_eq!(tokens[15], Token::Le);
        assert_eq!(tokens[16], Token::Gt);
        assert_eq!(tokens[17], Token::Ge);
        assert_eq!(tokens[18], Token::Shl);
        assert_eq!(tokens[19], Token::Shr);
        assert_eq!(tokens[20], Token::Eq);
        assert_eq!(tokens[21], Token::Arrow);
        assert_eq!(tokens[22], Token::FatArrow);
    }

    #[test]
    fn test_punctuation() {
        let input = "( ) { } [ ] ; , . .. : ::";
        let tokens = tokenize(input).unwrap();

        assert_eq!(tokens[0], Token::LeftParen);
        assert_eq!(tokens[1], Token::RightParen);
        assert_eq!(tokens[2], Token::LeftBrace);
        assert_eq!(tokens[3], Token::RightBrace);
        assert_eq!(tokens[4], Token::LeftBracket);
        assert_eq!(tokens[5], Token::RightBracket);
        assert_eq!(tokens[6], Token::Semicolon);
        assert_eq!(tokens[7], Token::Comma);
        assert_eq!(tokens[8], Token::Dot);
        assert_eq!(tokens[9], Token::DotDot);
        assert_eq!(tokens[10], Token::Colon);
        assert_eq!(tokens[11], Token::ColonColon);
    }

    #[test]
    fn test_booleans() {
        let input = "true false";
        let tokens = tokenize(input).unwrap();

        assert_eq!(tokens[0], Token::BoolLiteral(true));
        assert_eq!(tokens[1], Token::BoolLiteral(false));
    }

    #[test]
    fn test_comments() {
        let input = "foo // this is a comment\nbar";
        let tokens = tokenize(input).unwrap();

        assert_eq!(tokens[0], Token::Ident("foo".to_string()));
        assert_eq!(tokens[1], Token::Ident("bar".to_string()));
    }

    #[test]
    fn test_whitespace() {
        let input = "  foo  \t  bar\n  baz  ";
        let tokens = tokenize(input).unwrap();

        assert_eq!(tokens[0], Token::Ident("foo".to_string()));
        assert_eq!(tokens[1], Token::Ident("bar".to_string()));
        assert_eq!(tokens[2], Token::Ident("baz".to_string()));
    }

    #[test]
    fn test_use_syntax() {
        let input = "use crate::module::{Item1, Item2}";
        let tokens = tokenize(input).unwrap();

        assert_eq!(tokens[0], Token::Use);
        assert_eq!(tokens[1], Token::Crate);
        assert_eq!(tokens[2], Token::ColonColon);
        assert_eq!(tokens[3], Token::Ident("module".to_string()));
        assert_eq!(tokens[4], Token::ColonColon);
        assert_eq!(tokens[5], Token::LeftBrace);
        assert_eq!(tokens[6], Token::Ident("Item1".to_string()));
        assert_eq!(tokens[7], Token::Comma);
        assert_eq!(tokens[8], Token::Ident("Item2".to_string()));
        assert_eq!(tokens[9], Token::RightBrace);
    }

    #[test]
    fn test_error_unterminated_string() {
        let input = r#""unterminated"#;
        let result = tokenize(input);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            LexerError::UnterminatedString(_)
        ));
    }

    #[test]
    fn test_error_unexpected_char() {
        let input = "foo @ bar";
        let result = tokenize(input);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            LexerError::UnexpectedChar('@', _)
        ));
    }
}
