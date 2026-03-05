use std::error::Error;
use std::io::{self, BufRead};

use crate::emit;

pub fn run(service_name: &str) -> Result<(), Box<dyn Error>> {
    let stdin = io::stdin();
    let mut spans = Vec::new();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let span: serde_json::Value = serde_json::from_str(&line)?;
        spans.push(span);
    }

    if spans.is_empty() {
        return Ok(());
    }

    emit::traces(service_name, &spans);
    Ok(())
}
