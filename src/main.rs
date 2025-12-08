//! CLI entry point for the mdbook-typst-math preprocessor.

use std::{io, process};

use clap::{Parser, Subcommand};
use mdbook_preprocessor::{errors::Error, parse_input, Preprocessor};
use tracing::error;

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
    // Use tracing and tracing_subscriber to follow mdbook's style for logging
    let filter = tracing_subscriber::EnvFilter::builder()
        .with_env_var("MDBOOK_TYPST_MATH_LOG")
        .with_default_directive(tracing_subscriber::filter::LevelFilter::INFO.into())
        .from_env_lossy();
    let with_target = std::env::var("MDBOOK_TYPST_MATH_LOG").is_ok();

    tracing_subscriber::fmt()
        .without_time()
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .with_target(with_target)
        .init();

    let cli = Cli::parse();

    let pre = mdbook_typst_math::TypstProcessor;

    match cli.command {
        Some(Command::Supports { renderer }) => {
            handle_supports(&pre, &renderer);
        }
        None => handle_preprocess(&pre).unwrap_or_else(|e| {
            error!("{e}");
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
