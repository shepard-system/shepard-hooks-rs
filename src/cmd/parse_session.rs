use std::error::Error;

use crate::parsers;

pub fn run(provider: &str, file_path: &str) -> Result<(), Box<dyn Error>> {
    match provider {
        "claude" => parsers::claude::parse(file_path),
        "codex" => parsers::codex::parse(file_path),
        "gemini" => parsers::gemini::parse(file_path),
        _ => Err(format!("unknown provider: {provider}").into()),
    }
}
