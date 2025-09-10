use std::fs;
use std::path::Path;
use std::process::Command;

fn compile_file(input_path: &str) -> Result<String, String> {
    let output = Command::new("cargo")
        .args(&["run", "--", input_path])
        .output()
        .map_err(|e| format!("Failed to run compiler: {}", e))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    // Read the generated Lua file
    let lua_path = Path::new(input_path).with_extension("lua");
    fs::read_to_string(&lua_path).map_err(|e| format!("Failed to read generated Lua file: {}", e))
}

#[test]
fn compile_simple_program() {
    let source = r#"
fn main() {
    let x = 5;
    print(x);
}
"#;

    let test_file = "test_simple.rico8";
    fs::write(test_file, source).unwrap();

    let lua = compile_file(test_file).unwrap();

    assert!(lua.contains("function main()"));
    assert!(lua.contains("local x = 5"));
    assert!(lua.contains("print(x)"));

    // Cleanup
    fs::remove_file(test_file).ok();
    fs::remove_file("test_simple.lua").ok();
}

#[test]
fn compile_struct_example() {
    let source = r#"
struct Point {
    x: f32,
    y: f32,
}

impl Point {
    fn new(x: f32, y: f32) -> Point {
        Point { x: x, y: y }
    }
}

fn main() {
    let p = Point::new(3.0, 4.0);
    print(p.x);
}
"#;

    let test_file = "test_struct.rico8";
    fs::write(test_file, source).unwrap();

    let lua = compile_file(test_file).unwrap();

    assert!(lua.contains("Point = {}"));
    assert!(lua.contains("function Point:new(x, y)"));
    assert!(lua.contains("setmetatable(obj, {__index = Point})"));

    // Cleanup
    fs::remove_file(test_file).ok();
    fs::remove_file("test_struct.lua").ok();
}

#[test]
fn compile_enum_example() {
    let source = r#"
enum State {
    Idle,
    Running,
}

fn process(state: State) {
    match state {
        State::Idle => print("idle"),
        State::Running => print("running"),
    }
}
"#;

    let test_file = "test_enum.rico8";
    fs::write(test_file, source).unwrap();

    let lua = compile_file(test_file).unwrap();

    assert!(lua.contains("Idle = {"));
    assert!(lua.contains("tag = \"Idle\""));
    assert!(lua.contains("Running = {"));
    assert!(lua.contains("tag = \"Running\""));
    assert!(lua.contains("local __match = state"));

    // Cleanup
    fs::remove_file(test_file).ok();
    fs::remove_file("test_enum.lua").ok();
}

#[test]
fn compile_pico8_game() {
    let source = r#"
fn _init() {
    cls(0);
}

fn _update() {
    if btn(0) {
        print("left");
    }
}

fn _draw() {
    cls(1);
    circfill(64, 64, 10, 7);
}
"#;

    let test_file = "test_pico8.rico8";
    fs::write(test_file, source).unwrap();

    let lua = compile_file(test_file).unwrap();

    assert!(lua.contains("function _init()"));
    assert!(lua.contains("cls(0)"));
    assert!(lua.contains("function _update()"));
    assert!(lua.contains("btn(0)"));
    assert!(lua.contains("function _draw()"));
    assert!(lua.contains("circfill(64, 64, 10, 7)"));

    // Cleanup
    fs::remove_file(test_file).ok();
    fs::remove_file("test_pico8.lua").ok();
}

#[test]
fn compile_with_imports() {
    // Create a module file
    let module_source = r#"
pub struct Player {
    x: i32,
    y: i32,
}

impl Player {
    pub fn new() -> Player {
        Player { x: 64, y: 64 }
    }
}
"#;

    // Create main file that imports the module
    let main_source = r#"
use player::Player;

fn main() {
    let p = Player::new();
    print(p.x);
}
"#;

    fs::write("player.rico8", module_source).unwrap();
    fs::write("test_import.rico8", main_source).unwrap();

    let lua = compile_file("test_import.rico8").unwrap();

    // Should include the imported Player struct
    assert!(lua.contains("Player = {}"));
    assert!(lua.contains("function Player:new()"));

    // Cleanup
    fs::remove_file("player.rico8").ok();
    fs::remove_file("test_import.rico8").ok();
    fs::remove_file("test_import.lua").ok();
}

#[test]
fn compile_loops() {
    let source = r#"
fn test_loops() {
    for i in 0..5 {
        print(i);
    }
    
    let mut x = 0;
    while x < 3 {
        x = x + 1;
    }
}
"#;

    let test_file = "test_loops.rico8";
    fs::write(test_file, source).unwrap();

    let lua = compile_file(test_file).unwrap();

    assert!(lua.contains("for i=0,5"));
    assert!(lua.contains("while (x < 3)"));

    // Cleanup
    fs::remove_file(test_file).ok();
    fs::remove_file("test_loops.lua").ok();
}

#[test]
fn compile_arrays() {
    let source = r#"
fn test_arrays() {
    let arr = [1, 2, 3];
    let val = arr[0];
    arr[1] = 10;
}
"#;

    let test_file = "test_arrays.rico8";
    fs::write(test_file, source).unwrap();

    let lua = compile_file(test_file).unwrap();

    assert!(lua.contains("{1, 2, 3}"));
    assert!(lua.contains("arr[0]"));
    assert!(lua.contains("arr[1] = 10"));

    // Cleanup
    fs::remove_file(test_file).ok();
    fs::remove_file("test_arrays.lua").ok();
}

#[test]
fn compile_bitwise_ops() {
    let source = r#"
fn test_bitwise() {
    let a = 0xFF & 0x0F;
    let b = 0x01 | 0x02;
    let c = 0xFF ^ 0x0F;
}
"#;

    let test_file = "test_bitwise.rico8";
    fs::write(test_file, source).unwrap();

    let lua = compile_file(test_file).unwrap();

    assert!(lua.contains("band(255, 15)"));
    assert!(lua.contains("bor(1, 2)"));
    assert!(lua.contains("bxor(255, 15)"));

    // Cleanup
    fs::remove_file(test_file).ok();
    fs::remove_file("test_bitwise.lua").ok();
}

#[test]
fn compile_trait() {
    let source = r#"
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
"#;

    let test_file = "test_trait.rico8";
    fs::write(test_file, source).unwrap();

    let lua = compile_file(test_file).unwrap();

    assert!(lua.contains("-- trait Drawable"));
    assert!(lua.contains("Circle = {}"));
    assert!(lua.contains("-- impl Drawable for Circle"));
    assert!(lua.contains("function Circle:draw()"));

    // Cleanup
    fs::remove_file(test_file).ok();
    fs::remove_file("test_trait.lua").ok();
}

#[test]
fn compile_globals() {
    let source = r#"
const MAX_SCORE: i32 = 100;
let mut score: i32 = 0;

fn add_point() {
    score = score + 1;
    if score > MAX_SCORE {
        score = MAX_SCORE;
    }
}
"#;

    let test_file = "test_globals.rico8";
    fs::write(test_file, source).unwrap();

    let lua = compile_file(test_file).unwrap();

    assert!(lua.contains("MAX_SCORE = 100"));
    assert!(lua.contains("score = 0"));

    // Cleanup
    fs::remove_file(test_file).ok();
    fs::remove_file("test_globals.lua").ok();
}

#[test]
fn compile_string_concat() {
    let source = r#"
fn greet(name: String) -> String {
    return "Hello, " + name;
}
"#;

    let test_file = "test_strings.rico8";
    fs::write(test_file, source).unwrap();

    let lua = compile_file(test_file).unwrap();

    assert!(lua.contains("(\"Hello, \" .. name)"));

    // Cleanup
    fs::remove_file(test_file).ok();
    fs::remove_file("test_strings.lua").ok();
}

#[test]
fn compile_error_invalid_syntax() {
    let source = r#"
fn broken() {
    let x = ;  // Invalid syntax
}
"#;

    let test_file = "test_error.rico8";
    fs::write(test_file, source).unwrap();

    let result = compile_file(test_file);
    assert!(result.is_err());

    // Cleanup
    fs::remove_file(test_file).ok();
}

#[test]
fn compile_no_trailing_whitespace() {
    let source = r#"
struct Test {
    x: i32,
}

fn main() {
    let t = Test { x: 5 };
}
"#;

    let test_file = "test_whitespace.rico8";
    fs::write(test_file, source).unwrap();

    let lua = compile_file(test_file).unwrap();

    // Check no trailing whitespace
    for line in lua.lines() {
        assert!(
            !line.ends_with(' ') && !line.ends_with('\t'),
            "Found trailing whitespace in line: {:?}",
            line
        );
    }

    // Cleanup
    fs::remove_file(test_file).ok();
    fs::remove_file("test_whitespace.lua").ok();
}
