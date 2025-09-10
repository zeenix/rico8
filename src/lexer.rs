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
