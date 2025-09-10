// Validates that the generated Lua code has proper structure for Pico-8.
fn validate_pico8_structure(lua_code: &str) -> Result<(), String> {
    // Check for common Lua syntax errors that would break in Pico-8
    let mut paren_count = 0;
    let mut brace_count = 0;
    let mut in_string = false;
    let mut escape_next = false;

    for ch in lua_code.chars() {
        if escape_next {
            escape_next = false;
            continue;
        }

        if ch == '\\' && in_string {
            escape_next = true;
            continue;
        }

        if ch == '"' && !in_string {
            in_string = true;
        } else if ch == '"' && in_string {
            in_string = false;
        }

        if !in_string {
            match ch {
                '(' => paren_count += 1,
                ')' => {
                    paren_count -= 1;
                    if paren_count < 0 {
                        return Err("Unmatched closing parenthesis".to_string());
                    }
                }
                '{' => brace_count += 1,
                '}' => {
                    brace_count -= 1;
                    if brace_count < 0 {
                        return Err("Unmatched closing brace".to_string());
                    }
                }
                _ => {}
            }
        }
    }

    if paren_count != 0 {
        return Err(format!("Unclosed parentheses: {}", paren_count));
    }
    if brace_count != 0 {
        return Err(format!("Unclosed braces: {}", brace_count));
    }
    if in_string {
        return Err("Unclosed string".to_string());
    }

    Ok(())
}

#[test]
fn test_generated_lua_syntax() {
    use rico8::codegen::generate;
    use rico8::lexer::tokenize;
    use rico8::parser::parse;

    let test_cases = vec![
        // Simple function
        r#"
        fn main() {
            print("Hello");
        }
        "#,
        // Function with implicit return
        r#"
        fn add(a: i32, b: i32) -> i32 {
            a + b
        }
        "#,
        // Struct with methods
        r#"
        struct Point {
            x: i32,
            y: i32,
        }

        impl Point {
            fn new(x: i32, y: i32) -> Point {
                Point { x: x, y: y }
            }

            fn distance(&self) -> f32 {
                sqrt(self.x * self.x + self.y * self.y)
            }
        }
        "#,
        // Enum with match
        r#"
        enum State {
            Ready,
            Running,
        }

        fn process(s: State) {
            match s {
                State::Ready => print("ready"),
                State::Running => print("running"),
            }
        }
        "#,
        // Trait implementation
        r#"
        trait Drawable {
            fn draw(&self);
        }

        struct Circle {
            x: i32,
            y: i32,
            r: i32,
        }

        impl Drawable for Circle {
            fn draw(&self) {
                circfill(self.x, self.y, self.r, 7);
            }
        }
        "#,
        // Control flow
        r#"
        fn test() {
            if x > 0 {
                print("positive");
            } else {
                print("negative");
            }

            while y < 10 {
                y = y + 1;
            }

            for i in 0..10 {
                print(i);
            }
        }
        "#,
        // Arrays and indexing
        r#"
        fn test() {
            let arr = [1, 2, 3];
            let val = arr[0];
            arr[1] = 10;
        }
        "#,
        // Bitwise operations
        r#"
        fn test() {
            let a = 0xFF & 0x0F;
            let b = 0x01 | 0x02;
            let c = 0xFF ^ 0x0F;
        }
        "#,
        // Method calls
        r#"
        fn test() {
            player.update();
            let color = player.get_color();
        }
        "#,
        // Pico-8 functions
        r#"
        fn _init() {
            cls(0);
        }

        fn _update() {
            if btn(0) {
                x = x - 1;
            }
        }

        fn _draw() {
            cls(1);
            circfill(64, 64, 10, 7);
            rectfill(0, 0, 127, 8, 0);
            print("Hello", 0, 0, 7);
        }
        "#,
    ];

    for (i, source) in test_cases.iter().enumerate() {
        let tokens = tokenize(source).expect(&format!("Failed to tokenize test case {}", i));
        let ast = parse(tokens).expect(&format!("Failed to parse test case {}", i));
        let lua = generate(ast).expect(&format!("Failed to generate Lua for test case {}", i));

        // Check that the generated Lua has valid structure for Pico-8
        if let Err(e) = validate_pico8_structure(&lua) {
            panic!(
                "Test case {} generated invalid Pico-8 structure:\n{}\n\nError: {}",
                i, lua, e
            );
        }

        // Verify essential Pico-8 patterns are present
        assert!(
            lua.contains("function") || lua.contains("local"),
            "Test case {} should generate functions or variables",
            i
        );
    }
}

#[test]
fn test_pico8_specific_syntax() {
    use rico8::codegen::generate;
    use rico8::lexer::tokenize;
    use rico8::parser::parse;

    // Test Pico-8 specific functions and patterns
    let source = r#"
        fn _init() {
            music(0, 0, 0);
        }

        fn _update() {
            if btnp(4, 0) || btnp(5, 0) {
                sfx(1, -1, 0, 0);
            }
        }

        fn _draw() {
            cls(1);

            // Draw sprites
            spr(0, 64, 64);

            // Draw shapes
            circfill(64, 64, 10, 7);
            rectfill(0, 0, 127, 8, 0);
            rect(20, 20, 100, 100, 12);
            line(0, 0, 127, 127, 7);
            pset(64, 64, 8);

            // Print text
            print("Score: " + tostr(score, false), 2, 2, 7);

            // Camera and palette
            camera(0, 0);
            pal(7, 12);

            // Memory operations
            poke(0x5f2c, 3);
            let val = peek(0x5f2c);
        }
    "#;

    let tokens = tokenize(source).expect("Failed to tokenize Pico-8 code");
    let ast = parse(tokens).expect("Failed to parse Pico-8 code");
    let lua = generate(ast).expect("Failed to generate Lua for Pico-8 code");

    // Verify the generated Lua contains Pico-8 API calls
    assert!(lua.contains("music(0, 0, 0)"));
    assert!(lua.contains("btnp(4, 0)"));
    assert!(lua.contains("sfx(1, -1, 0, 0)"));
    assert!(lua.contains("cls(1)"));
    assert!(lua.contains("spr(0, 64, 64)"));
    assert!(lua.contains("circfill(64, 64, 10, 7)"));
    assert!(lua.contains("rectfill(0, 0, 127, 8, 0)"));
    assert!(lua.contains("rect(20, 20, 100, 100, 12)"));
    assert!(lua.contains("line(0, 0, 127, 127, 7)"));
    assert!(lua.contains("pset(64, 64, 8)"));
    assert!(lua.contains("camera(0, 0)"));
    assert!(lua.contains("pal(7, 12)"));
    // 0x5f2c = 24364 in decimal
    assert!(lua.contains("poke(24364, 3)"));
    assert!(lua.contains("peek(24364)"));

    // Validate structure for Pico-8
    if let Err(e) = validate_pico8_structure(&lua) {
        panic!(
            "Generated invalid structure for Pico-8 code:\n{}\n\nError: {}",
            lua, e
        );
    }
}

#[test]
fn test_complete_game_example() {
    use rico8::codegen::generate;
    use rico8::lexer::tokenize;
    use rico8::parser::parse;

    let source = r#"
        struct Player {
            x: i32,
            y: i32,
            dx: i32,
            dy: i32,
        }

        impl Player {
            fn new() -> Player {
                Player { x: 64, y: 64, dx: 0, dy: 0 }
            }

            fn update(&mut self) {
                if btn(0, 0) {
                    self.dx = -2;
                } else if btn(1, 0) {
                    self.dx = 2;
                } else {
                    self.dx = self.dx * 0.8;
                }

                self.x = self.x + self.dx;

                if self.x < 4 {
                    self.x = 4;
                }
                if self.x > 120 {
                    self.x = 120;
                }
            }

            fn draw(&self) {
                circfill(self.x, self.y, 3, 12);
            }
        }

        let mut player: Player;

        fn _init() {
            player = Player::new();
        }

        fn _update() {
            player.update();
        }

        fn _draw() {
            cls(1);
            player.draw();
            print("Use arrows to move", 24, 8, 7);
        }
    "#;

    let tokens = tokenize(source).expect("Failed to tokenize game example");
    let ast = parse(tokens).expect("Failed to parse game example");
    let lua = generate(ast).expect("Failed to generate Lua for game example");

    // Verify the game structure is correct
    assert!(lua.contains("Player = {}"));
    assert!(lua.contains("function Player:new()"));
    assert!(lua.contains("function Player:update()"));
    assert!(lua.contains("function Player:draw()"));
    assert!(lua.contains("function _init()"));
    assert!(lua.contains("function _update()"));
    assert!(lua.contains("function _draw()"));

    // Verify player instance creation
    assert!(lua.contains("player = Player:new()"));
    assert!(lua.contains("player:update()"));
    assert!(lua.contains("player:draw()"));

    // Validate structure for Pico-8
    if let Err(e) = validate_pico8_structure(&lua) {
        panic!(
            "Generated invalid structure for game example:\n{}\n\nError: {}",
            lua, e
        );
    }
}
