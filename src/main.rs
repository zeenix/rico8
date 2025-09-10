use anyhow::Result;
use clap::Parser;
use std::fs;
use std::path::PathBuf;

mod ast;
mod codegen;
mod lexer;
mod parser;

#[derive(Parser)]
#[command(name = "rico8")]
#[command(about = "A Rust-like language that transpiles to Pico-8 Lua")]
struct Cli {
    #[arg(help = "Input Rico8 source file")]
    input: PathBuf,

    #[arg(
        short,
        long,
        help = "Output Lua file (defaults to input with .lua extension)"
    )]
    output: Option<PathBuf>,

    #[arg(short, long, help = "Verbose output")]
    verbose: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let source = fs::read_to_string(&cli.input)?;

    if cli.verbose {
        eprintln!("Transpiling {}...", cli.input.display());
    }

    let tokens = match lexer::tokenize(&source) {
        Ok(tokens) => tokens,
        Err(e) => {
            eprintln!("Lexer error: {}", e);
            return Err(e.into());
        }
    };

    if cli.verbose {
        eprintln!("Lexed {} tokens", tokens.len());
        // Debug specific position
        for (i, token) in tokens.iter().enumerate() {
            if i >= 1559 && i <= 1569 {
                eprintln!("Token {}: {:?}", i, token);
            }
        }
    }

    let ast = match parser::parse(tokens) {
        Ok(ast) => ast,
        Err(e) => {
            eprintln!("Parser error: {}", e);
            return Err(e.into());
        }
    };

    let lua_code = codegen::generate(ast)?;

    let output_path = cli.output.unwrap_or_else(|| {
        let mut path = cli.input.clone();
        path.set_extension("lua");
        path
    });

    fs::write(&output_path, lua_code)?;

    if cli.verbose {
        eprintln!("Successfully wrote to {}", output_path.display());
    }

    Ok(())
}
