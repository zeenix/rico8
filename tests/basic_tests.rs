use rico8::codegen::generate;
use rico8::lexer::tokenize;
use rico8::parser::parse;

fn compile_source(input: &str) -> Result<String, String> {
    let tokens = tokenize(input).map_err(|e| e.to_string())?;
    let ast = parse(tokens).map_err(|e| e.to_string())?;
    generate(ast).map_err(|e| e.to_string())
}

#[test]
fn test_simple_function() {
    let source = r#"
        fn main() {
            print("Hello");
        }
    "#;

    let result = compile_source(source);
    assert!(result.is_ok());
    let lua = result.unwrap();
    assert!(lua.contains("function main()"));
    assert!(lua.contains("print(\"Hello\")"));
}

#[test]
fn test_struct() {
    let source = r#"
        struct Point {
            x: i32,
            y: i32,
        }
    "#;

    let result = compile_source(source);
    assert!(result.is_ok());
    let lua = result.unwrap();
    assert!(lua.contains("Point = {}"));
}

#[test]
fn test_enum() {
    let source = r#"
        enum State {
            Ready,
            Running,
        }
    "#;

    let result = compile_source(source);
    assert!(result.is_ok());
    let lua = result.unwrap();
    assert!(lua.contains("Ready = {"));
    assert!(lua.contains("tag = \"Ready\""));
}

#[test]
fn test_impl() {
    let source = r#"
        struct Player {
            x: i32,
        }
        
        impl Player {
            fn new() -> Player {
                Player { x: 0 }
            }
        }
    "#;

    let result = compile_source(source);
    assert!(result.is_ok(), "Compilation failed: {:?}", result.err());
    let lua = result.unwrap();
    assert!(lua.contains("function Player:new()"));
}

#[test]
fn test_trait() {
    let source = r#"
        trait Drawable {
            fn draw(&self);
        }
    "#;

    let result = compile_source(source);
    assert!(result.is_ok());
    let lua = result.unwrap();
    assert!(lua.contains("-- trait Drawable"));
}

#[test]
fn test_const() {
    let source = r#"
        const MAX: i32 = 100;
    "#;

    let result = compile_source(source);
    assert!(result.is_ok());
    let lua = result.unwrap();
    assert!(lua.contains("MAX = 100"));
}

#[test]
fn test_let() {
    let source = r#"
        fn test() {
            let x = 5;
            let mut y = 10;
        }
    "#;

    let result = compile_source(source);
    assert!(result.is_ok());
    let lua = result.unwrap();
    assert!(lua.contains("local x = 5"));
    assert!(lua.contains("local y = 10"));
}

#[test]
fn test_if_else() {
    let source = r#"
        fn test() {
            if x > 0 {
                print("positive");
            } else {
                print("negative");
            }
        }
    "#;

    let result = compile_source(source);
    assert!(result.is_ok());
    let lua = result.unwrap();
    assert!(lua.contains("if (x > 0)"));
}

#[test]
fn test_while_loop() {
    let source = r#"
        fn test() {
            while x < 10 {
                x = x + 1;
            }
        }
    "#;

    let result = compile_source(source);
    assert!(result.is_ok());
    let lua = result.unwrap();
    assert!(lua.contains("while (x < 10)"));
}

#[test]
fn test_for_loop() {
    let source = r#"
        fn test() {
            for i in 0..10 {
                print(i);
            }
        }
    "#;

    let result = compile_source(source);
    assert!(result.is_ok());
    let lua = result.unwrap();
    assert!(lua.contains("for i=0,10"));
}

#[test]
fn test_match() {
    let source = r#"
        enum State { Ready, Running }
        
        fn test(s: State) {
            match s {
                State::Ready => print("ready"),
                State::Running => print("running"),
            }
        }
    "#;

    let result = compile_source(source);
    assert!(result.is_ok());
    let lua = result.unwrap();
    assert!(lua.contains("local __match = s"));
}

#[test]
fn test_arrays() {
    let source = r#"
        fn test() {
            let arr = [1, 2, 3];
            let val = arr[0];
        }
    "#;

    let result = compile_source(source);
    assert!(result.is_ok());
    let lua = result.unwrap();
    assert!(lua.contains("{1, 2, 3}"));
    assert!(lua.contains("arr[0]"));
}

#[test]
fn test_method_call() {
    let source = r#"
        fn test() {
            player.update();
        }
    "#;

    let result = compile_source(source);
    assert!(result.is_ok());
    let lua = result.unwrap();
    assert!(lua.contains("player:update()"));
}

#[test]
fn test_bitwise() {
    let source = r#"
        fn test() {
            let a = 0xFF & 0x0F;
            let b = 0x01 | 0x02;
            let c = 0xFF ^ 0x0F;
        }
    "#;

    let result = compile_source(source);
    assert!(result.is_ok(), "Compilation failed: {:?}", result.err());
    let lua = result.unwrap();
    assert!(lua.contains("band("));
    assert!(lua.contains("bor("));
    assert!(lua.contains("bxor("));
}

#[test]
fn test_pico8_functions() {
    let source = r#"
        fn _init() {
            cls(0);
        }
        
        fn _update() {
            if btn(0) {
                x = x - 1;
            }
        }
        
        fn _draw() {
            circfill(64, 64, 10, 7);
        }
    "#;

    let result = compile_source(source);
    assert!(result.is_ok());
    let lua = result.unwrap();
    assert!(lua.contains("function _init()"));
    assert!(lua.contains("function _update()"));
    assert!(lua.contains("function _draw()"));
}

#[test]
fn test_use_statement() {
    let source = r#"
        use std::collections::HashMap;
    "#;

    let tokens = tokenize(source);
    assert!(tokens.is_ok());
    let ast = parse(tokens.unwrap());
    assert!(ast.is_ok());
    let program = ast.unwrap();
    assert_eq!(program.imports.len(), 1);
}

#[test]
fn test_no_trailing_whitespace() {
    let source = r#"
        fn test() {
            let x = 5;
        }
    "#;

    let result = compile_source(source);
    assert!(result.is_ok());
    let lua = result.unwrap();

    for line in lua.lines() {
        assert!(
            !line.ends_with(' ') && !line.ends_with('\t'),
            "Found trailing whitespace in line: {:?}",
            line
        );
    }
}
