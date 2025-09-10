use rico8::lexer::{tokenize, Token};

#[test]
fn keywords() {
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
fn identifiers() {
    let input = "player x123 _test camelCase snake_case CONST";
    let tokens = tokenize(input).unwrap();
    assert_eq!(tokens[0], Token::Ident("player".to_string()));
    assert_eq!(tokens[1], Token::Ident("x123".to_string()));
    assert_eq!(tokens[2], Token::Ident("_test".to_string()));
    assert_eq!(tokens[3], Token::Ident("camelCase".to_string()));
    assert_eq!(tokens[4], Token::Ident("snake_case".to_string()));
    assert_eq!(tokens[5], Token::Ident("CONST".to_string()));
}

#[test]
fn numbers() {
    let input = "0 42 123.456 0.5 1000 3.14159";
    let tokens = tokenize(input).unwrap();
    assert_eq!(tokens[0], Token::IntLiteral(0));
    assert_eq!(tokens[1], Token::IntLiteral(42));
    assert_eq!(tokens[2], Token::FloatLiteral(123.456));
    assert_eq!(tokens[3], Token::FloatLiteral(0.5));
    assert_eq!(tokens[4], Token::IntLiteral(1000));
    assert_eq!(tokens[5], Token::FloatLiteral(3.14159));
}

#[test]
fn hexadecimal_numbers() {
    let input = "0xFF 0x00 0x0F 0x01 0x7FFFFFFF";
    let tokens = tokenize(input).unwrap();
    assert_eq!(tokens[0], Token::IntLiteral(0xFF));
    assert_eq!(tokens[1], Token::IntLiteral(0x00));
    assert_eq!(tokens[2], Token::IntLiteral(0x0F));
    assert_eq!(tokens[3], Token::IntLiteral(0x01));
    assert_eq!(tokens[4], Token::IntLiteral(0x7FFFFFFF));
}

#[test]
fn strings() {
    let input = r#""hello" "world" "with spaces" "with \"quotes\"" "" "123""#;
    let tokens = tokenize(input).unwrap();
    assert_eq!(tokens[0], Token::StringLiteral("hello".to_string()));
    assert_eq!(tokens[1], Token::StringLiteral("world".to_string()));
    assert_eq!(tokens[2], Token::StringLiteral("with spaces".to_string()));
    assert_eq!(
        tokens[3],
        Token::StringLiteral("with \"quotes\"".to_string())
    );
    assert_eq!(tokens[4], Token::StringLiteral("".to_string()));
    assert_eq!(tokens[5], Token::StringLiteral("123".to_string()));
}

#[test]
fn chars() {
    let input = "'a' 'Z' '0' ' ' '\\n'";
    let tokens = tokenize(input).unwrap();
    assert_eq!(tokens[0], Token::CharLiteral('a'));
    assert_eq!(tokens[1], Token::CharLiteral('Z'));
    assert_eq!(tokens[2], Token::CharLiteral('0'));
    assert_eq!(tokens[3], Token::CharLiteral(' '));
    assert_eq!(tokens[4], Token::CharLiteral('\n'));
}

#[test]
fn operators() {
    let input = "+ - * / % = == != < > <= >= && || ! & | ^";
    let tokens = tokenize(input).unwrap();
    assert_eq!(tokens[0], Token::Plus);
    assert_eq!(tokens[1], Token::Minus);
    assert_eq!(tokens[2], Token::Star);
    assert_eq!(tokens[3], Token::Slash);
    assert_eq!(tokens[4], Token::Percent);
    assert_eq!(tokens[5], Token::Eq);
    assert_eq!(tokens[6], Token::EqEq);
    assert_eq!(tokens[7], Token::Ne);
    assert_eq!(tokens[8], Token::Lt);
    assert_eq!(tokens[9], Token::Gt);
    assert_eq!(tokens[10], Token::Le);
    assert_eq!(tokens[11], Token::Ge);
    assert_eq!(tokens[12], Token::AndAnd);
    assert_eq!(tokens[13], Token::OrOr);
    assert_eq!(tokens[14], Token::Bang);
    assert_eq!(tokens[15], Token::Ampersand);
    assert_eq!(tokens[16], Token::Pipe);
    assert_eq!(tokens[17], Token::Caret);
}

#[test]
fn punctuation() {
    let input = "( ) { } [ ] , . : ; :: -> => ..";
    let tokens = tokenize(input).unwrap();
    assert_eq!(tokens[0], Token::LeftParen);
    assert_eq!(tokens[1], Token::RightParen);
    assert_eq!(tokens[2], Token::LeftBrace);
    assert_eq!(tokens[3], Token::RightBrace);
    assert_eq!(tokens[4], Token::LeftBracket);
    assert_eq!(tokens[5], Token::RightBracket);
    assert_eq!(tokens[6], Token::Comma);
    assert_eq!(tokens[7], Token::Dot);
    assert_eq!(tokens[8], Token::Colon);
    assert_eq!(tokens[9], Token::Semicolon);
    assert_eq!(tokens[10], Token::ColonColon);
    assert_eq!(tokens[11], Token::Arrow);
    assert_eq!(tokens[12], Token::FatArrow);
    assert_eq!(tokens[13], Token::DotDot);
    // DotDotDot not in current lexer
}

#[test]
fn booleans() {
    let input = "true false None Some";
    let tokens = tokenize(input).unwrap();
    assert_eq!(tokens[0], Token::BoolLiteral(true));
    assert_eq!(tokens[1], Token::BoolLiteral(false));
    assert_eq!(tokens[2], Token::Ident("None".to_string()));
    assert_eq!(tokens[3], Token::Ident("Some".to_string()));
}

#[test]
fn comments() {
    let input = "x // comment\ny";
    let tokens = tokenize(input).unwrap();
    assert_eq!(tokens[0], Token::Ident("x".to_string()));
    assert_eq!(tokens[1], Token::Ident("y".to_string()));
}

#[test]
fn whitespace() {
    let input = "  x  \n  y\t\tz  \r\n  ";
    let tokens = tokenize(input).unwrap();
    assert_eq!(tokens[0], Token::Ident("x".to_string()));
    assert_eq!(tokens[1], Token::Ident("y".to_string()));
    assert_eq!(tokens[2], Token::Ident("z".to_string()));
}

#[test]
fn use_syntax() {
    let input = "use std::collections::HashMap; use crate::*;";
    let tokens = tokenize(input).unwrap();
    assert_eq!(tokens[0], Token::Use);
    assert_eq!(tokens[1], Token::Ident("std".to_string()));
    assert_eq!(tokens[2], Token::ColonColon);
    assert_eq!(tokens[3], Token::Ident("collections".to_string()));
    assert_eq!(tokens[4], Token::ColonColon);
    assert_eq!(tokens[5], Token::Ident("HashMap".to_string()));
    assert_eq!(tokens[6], Token::Semicolon);
    assert_eq!(tokens[7], Token::Use);
    assert_eq!(tokens[8], Token::Crate);
    assert_eq!(tokens[9], Token::ColonColon);
    assert_eq!(tokens[10], Token::Star);
    assert_eq!(tokens[11], Token::Semicolon);
}

#[test]
fn error_unclosed_string() {
    let input = r#""unclosed string"#;
    let result = tokenize(input);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Unterminated string"));
}

#[test]
fn error_invalid_char() {
    let input = "'ab'";
    let result = tokenize(input);
    assert!(result.is_err());
}

#[test]
fn error_unclosed_char() {
    let input = "'a";
    let result = tokenize(input);
    assert!(result.is_err());
}
