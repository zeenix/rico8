use anyhow::Result;
use clap::Parser;
use std::fs;
use std::path::PathBuf;

mod ast;
mod codegen;
mod lexer;
mod module_loader;
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

    if cli.verbose {
        eprintln!("Transpiling {}...", cli.input.display());
    }

    // Use module loader to handle imports
    let base_path = cli
        .input
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf();
    let mut loader = module_loader::ModuleLoader::new(base_path);

    let ast = match loader.load_program(&cli.input) {
        Ok(ast) => ast,
        Err(e) => {
            eprintln!("Module loading error: {}", e);
            return Err(e.into());
        }
    };

    if cli.verbose {
        eprintln!(
            "Loaded program with {} imports and {} items",
            ast.imports.len(),
            ast.items.len()
        );
    }

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
