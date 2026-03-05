use clap::{Parser, Subcommand};

mod cmd;
mod parsers;

#[derive(Parser)]
#[command(name = "shepard-hook", about = "Rust accelerator for shepard-obs-stack hooks")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Parse a CLI session file into JSONL spans on stdout
    ParseSession {
        /// Provider: claude, codex, gemini
        provider: String,
        /// Path to session log file
        file_path: String,
    },
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::ParseSession {
            provider,
            file_path,
        } => cmd::parse_session::run(&provider, &file_path),
    };

    if let Err(e) = result {
        eprintln!("shepard-hook: {e}");
        std::process::exit(1);
    }
}
