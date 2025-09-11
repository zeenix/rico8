#[test]
fn test_direction_enum_in_trait() {
    let source = r#"
        enum Direction {
            Left,
            Right,
            Up,
            Down,
        }

        trait Movable {
            fn get_x(&self) -> f32;
            fn get_y(&self) -> f32;
            fn set_x(&mut self, x: f32);
            fn set_y(&mut self, y: f32);

            fn move_dir(&mut self, dir: Direction, speed: f32) {
                let x = self.get_x();
                let y = self.get_y();

                match dir {
                    Direction::Left => self.set_x(x - speed),
                    Direction::Right => self.set_x(x + speed),
                    Direction::Up => self.set_y(y - speed),
                    Direction::Down => self.set_y(y + speed),
                }
            }
        }

        struct Player {
            x: f32,
            y: f32,
        }

        impl Movable for Player {
            fn get_x(&self) -> f32 { self.x }
            fn get_y(&self) -> f32 { self.y }
            fn set_x(&mut self, x: f32) { self.x = x; }
            fn set_y(&mut self, y: f32) { self.y = y; }
        }

        fn update_player(p: &mut Player) {
            p.move_dir(Direction::Left, 10.0);
        }
    "#;

    let tokens = rico8::lexer::tokenize(source).expect("Lexer error");
    let program = rico8::parser::parse(tokens).expect("Parser error");
    let lua = rico8::codegen::generate(program).expect("Codegen error");

    // Check Direction enum is generated
    assert!(lua.contains("Left = {\n  tag = \"Left\"\n}"));
    assert!(lua.contains("Right = {\n  tag = \"Right\"\n}"));
    assert!(lua.contains("Up = {\n  tag = \"Up\"\n}"));
    assert!(lua.contains("Down = {\n  tag = \"Down\"\n}"));

    // Check match in default method
    assert!(lua.contains("__match.tag == \"Left\""));
    assert!(lua.contains("__match.tag == \"Right\""));

    // Check trait method call
    assert!(lua.contains("p:move_dir(Left, 10)"));
}
