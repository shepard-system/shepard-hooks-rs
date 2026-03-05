use std::error::Error;
use std::io::{self, BufRead};

use crate::otlp;

const COLLECTOR_URL: &str = "http://localhost:4318/v1/traces";

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

    let payload = otlp::build_trace_export(service_name, &spans);

    reqwest::blocking::Client::new()
        .post(COLLECTOR_URL)
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&payload)?)
        .send()?;

    Ok(())
}
