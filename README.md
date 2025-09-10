# Rico8

Rico8 is a Rust-like language that transpiles to Pico-8 Lua. It provides type safety and familiar Rust syntax 
while generating clean Lua code for Pico-8 game development.

## Features

- **Rust-like syntax**: Structs, enums, traits, and impl blocks
- **Type safety**: Static typing with type inference
- **Pattern matching**: Match expressions with exhaustive patterns
- **Generics**: Basic generic type support
- **Method syntax**: Object-oriented style with self/&self/&mut self
- **Transpiles to Lua**: Generates clean, readable Pico-8 Lua code

## Language Features

### Structs
```rust
struct Player {
    x: i32,
    y: i32,
    health: i32,
}
```

### Enums
```rust
enum GameState {
    Menu,
    Playing,
    GameOver,
}
```

### Impl Blocks
```rust
impl Player {
    fn new(x: i32, y: i32) {
        return Player { x: x, y: y, health: 100 };
    }
    
    fn take_damage(&mut self, amount: i32) {
        self.health = self.health - amount;
    }
}
```

### Pattern Matching
```rust
match state {
    GameState::Menu => show_menu(),
    GameState::Playing => update_game(),
    GameState::GameOver => show_game_over(),
}
```

## Usage

```bash
# Transpile a Rico8 file to Lua
rico8 game.rico8

# Specify output file
rico8 game.rico8 -o game.lua

# Verbose output
rico8 game.rico8 -v
```

## Building

```bash
cargo build --release
```

## Examples

See the `examples/` directory for sample Rico8 programs.

## Pico-8 API Support

The project will include wrappers for the complete Pico-8 API, allowing you to use Pico-8 functions with 
proper type checking.

## License

MIT OR Apache-2.0