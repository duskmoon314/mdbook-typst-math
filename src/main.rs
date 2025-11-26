//! CLI entry point for the mdbook-typst-math preprocessor.

use std::{io, process};

use clap::{Parser, Subcommand};
use mdbook_preprocessor::{errors::Error, parse_input, Preprocessor};

#[derive(Parser, Debug)]
#[clap(author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Check whether a renderer is supported by mdbook-typst-math preprocessor
    Supports {
        /// The renderer to check support for
        renderer: String,
    },
}

fn main() {
    let cli = Cli::parse();

    let pre = mdbook_typst_math::TypstProcessor;

    match cli.command {
        Some(Command::Supports { renderer }) => {
            handle_supports(&pre, &renderer);
        }
        None => handle_preprocess(&pre).unwrap_or_else(|e| {
            eprintln!("Error: {}", e);
            process::exit(1);
        }),
    }
}

/// Checks if the preprocessor supports the given renderer and exits with appropriate code.
fn handle_supports(pre: &dyn Preprocessor, renderer: &str) {
    let supported = pre.supports_renderer(renderer).unwrap_or(false);
    process::exit(if supported { 0 } else { 1 });
}

/// Runs the preprocessor on stdin and writes the result to stdout.
fn handle_preprocess(pre: &dyn Preprocessor) -> Result<(), Error> {
    let (ctx, book) = parse_input(io::stdin())?;

    let processed_book = pre.run(&ctx, book)?;
    serde_json::to_writer(io::stdout(), &processed_book)?;

    Ok(())
}
