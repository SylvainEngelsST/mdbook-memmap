use anyhow::Result;
use clap::{Parser, Subcommand};
use mdbook_memmap::{install, Memmap};
use mdbook_preprocessor::Preprocessor;
use std::io;
use std::path::PathBuf;

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Check whether the preprocessor supports a renderer.
    Supports { renderer: String },
    /// Add preprocessor settings to an existing book without overwriting them.
    Install {
        #[arg(default_value = ".")]
        dir: PathBuf,
    },
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Some(Command::Supports { renderer }) => {
            if !Memmap.supports_renderer(&renderer)? {
                std::process::exit(1);
            }
        }
        Some(Command::Install { dir }) => {
            if install::install(&dir)? {
                eprintln!("mdbook-memmap: installed configuration");
            } else {
                eprintln!("mdbook-memmap: existing configuration left unchanged");
            }
        }
        None => {
            let (ctx, book) = mdbook_preprocessor::parse_input(io::stdin())?;
            let output = Memmap.run(&ctx, book)?;
            let bytes = serde_json::to_vec(&output)?;
            use io::Write;
            io::stdout().write_all(&bytes)?;
        }
    }
    Ok(())
}

fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("mdbook-memmap: {error:#}");
        std::process::exit(1);
    }
}
