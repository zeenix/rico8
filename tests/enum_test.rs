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
