use std::{io, process};

use clap::{Parser, Subcommand};
use mdbook::{
    errors::Error,
    preprocess::{CmdPreprocessor, Preprocessor},
};

#[derive(Parser, Debug)]
#[clap(author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    Supports {
        /// Check whether a renderer is supported by mdbook-typst-preprocessor
        renderer: String,
    },
}

fn main() {
    let cli = Cli::parse();

    let pre = mdbook_typst::TypstProcessor;

    match cli.command {
        Some(Command::Supports { .. }) => {
            handle_supports(&pre, &cli);
        }
        None => handle_preprocess(&pre).unwrap_or_else(|e| {
            eprintln!("Error: {}", e);
            process::exit(1);
        }),
    }
}

fn handle_supports(pre: &dyn Preprocessor, cli: &Cli) {
    if let Some(Command::Supports { renderer }) = &cli.command {
        let supported = pre.supports_renderer(renderer);

        if supported {
            process::exit(0);
        } else {
            process::exit(1);
        }
    } else {
        unreachable!("handle_supports called without supports subcommand")
    }
}

fn handle_preprocess(pre: &dyn Preprocessor) -> Result<(), Error> {
    let (ctx, book) = CmdPreprocessor::parse_input(io::stdin())?;

    let processed_book = pre.run(&ctx, book)?;
    serde_json::to_writer(io::stdout(), &processed_book)?;

    Ok(())
}
