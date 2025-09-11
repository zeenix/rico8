#[test]
fn test_enum_in_trait() {
    let source = r#"
        enum EntityType {
            Player,
            Enemy,
            Bullet,
        }

        trait Entity {
            fn get_type(&self) -> EntityType;

            fn is_player(&self) -> bool {
                match self.get_type() {
                    EntityType::Player => true,
                    _ => false,
                }
            }
        }

        struct Hero {}

        impl Entity for Hero {
            fn get_type(&self) -> EntityType {
                EntityType::Player
            }
        }
    "#;

    let tokens = rico8::lexer::tokenize(source).expect("Lexer error");
    let program = rico8::parser::parse(tokens).expect("Parser error");
    let lua = rico8::codegen::generate(program).expect("Codegen error");

    // Check enum variants are generated
    assert!(lua.contains("Player = {\n  tag = \"Player\"\n}"));
    assert!(lua.contains("Enemy = {\n  tag = \"Enemy\"\n}"));
    assert!(lua.contains("Bullet = {\n  tag = \"Bullet\"\n}"));

    // Check match expression in default method
    assert!(lua.contains("__match.tag == \"Player\""));

    // Check get_type returns enum variant
    assert!(lua.contains("return Player"));
}

#[test]
fn test_enum_match_exhaustive() {
    let source = r#"
        enum Color {
            Red,
            Green,
            Blue,
        }

        fn color_name(c: Color) -> &str {
            match c {
                Color::Red => "red",
                Color::Green => "green",
                Color::Blue => "blue",
            }
        }
    "#;

    let tokens = rico8::lexer::tokenize(source).expect("Lexer error");
    let program = rico8::parser::parse(tokens).expect("Parser error");
    let lua = rico8::codegen::generate(program).expect("Codegen error");

    // Check all branches are generated
    assert!(lua.contains("__match.tag == \"Red\""));
    assert!(lua.contains("__match.tag == \"Green\""));
    assert!(lua.contains("__match.tag == \"Blue\""));
    assert!(lua.contains("\"red\""));
    assert!(lua.contains("\"green\""));
    assert!(lua.contains("\"blue\""));
}

#[test]
fn test_enum_in_struct() {
    let source = r#"
        enum Status {
            Active,
            Inactive,
            Pending,
        }

        struct Task {
            name: &str,
            status: Status,
        }

        impl Task {
            fn is_active(&self) -> bool {
                match self.status {
                    Status::Active => true,
                    _ => false,
                }
            }
        }
    "#;

    let tokens = rico8::lexer::tokenize(source).expect("Lexer error");
    let program = rico8::parser::parse(tokens).expect("Parser error");
    let lua = rico8::codegen::generate(program).expect("Codegen error");

    // Check enum is generated
    assert!(lua.contains("Active = {\n  tag = \"Active\"\n}"));
    assert!(lua.contains("Inactive = {\n  tag = \"Inactive\"\n}"));
    assert!(lua.contains("Pending = {\n  tag = \"Pending\"\n}"));

    // Check match in method
    assert!(lua.contains("__match.tag == \"Active\""));
}
