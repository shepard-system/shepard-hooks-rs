use clap::{Parser, Subcommand};

mod cmd;
mod emit;
mod git_context;
mod hooks;
mod otlp;
mod parsers;
mod sensitive;

#[derive(Parser)]
#[command(
    name = "shepard-hook",
    about = "Rust accelerator for shepard-obs-stack hooks",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Emit an OTLP counter metric to the collector
    EmitMetric {
        /// Metric name (e.g. tool_calls_total)
        name: String,
        /// Counter value
        value: f64,
        /// JSON object with labels (e.g. '{"source":"claude","tool":"Read"}')
        labels: String,
    },

    /// Read JSONL spans from stdin and POST as OTLP traces
    EmitTraces {
        /// Service name (e.g. claude-code-session)
        service_name: String,
    },

    /// Parse a CLI session file into JSONL spans on stdout
    ParseSession {
        /// Provider: claude, codex, gemini
        provider: String,
        /// Path to session log file
        file_path: String,
    },

    /// Run a full hook (parse stdin + emit metrics + parse session)
    Hook {
        /// Provider: claude, codex, gemini
        provider: String,
        /// Hook name (e.g. post-tool-use, stop, notify)
        hook_name: String,
    },
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::EmitMetric {
            name,
            value,
            labels,
        } => cmd::emit_metric::run(&name, value, &labels),
        Commands::EmitTraces { service_name } => cmd::emit_traces::run(&service_name),
        Commands::ParseSession {
            provider,
            file_path,
        } => cmd::parse_session::run(&provider, &file_path),
        Commands::Hook {
            provider,
            hook_name,
        } => cmd::hook::run(&provider, &hook_name),
    };

    if let Err(e) = result {
        eprintln!("shepard-hook: {e}");
        std::process::exit(1);
    }
}
