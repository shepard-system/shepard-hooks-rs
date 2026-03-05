use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::otlp;

/// Read OTEL_HTTP_URL env var, default to localhost:4318.
fn collector_base() -> String {
    std::env::var("OTEL_HTTP_URL").unwrap_or_else(|_| "http://localhost:4318".to_string())
}

/// Fire-and-forget metric POST. Errors logged to stderr, never propagated.
pub fn metric(name: &str, value: f64, labels: &HashMap<String, String>) {
    let now_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos().to_string())
        .unwrap_or_else(|_| "0".to_string());

    let payload = otlp::build_sum_metric(name, value, labels, &now_nanos);
    let url = format!("{}/v1/metrics", collector_base());

    if let Err(e) = post_json(&url, &payload) {
        eprintln!("shepard-hook: metric emit failed: {e}");
    }
}

/// Fire-and-forget trace POST from span Vec. Errors logged to stderr.
pub fn traces(service_name: &str, spans: &[serde_json::Value]) {
    if spans.is_empty() {
        return;
    }

    let payload = otlp::build_trace_export(service_name, spans);
    let url = format!("{}/v1/traces", collector_base());

    if let Err(e) = post_json(&url, &payload) {
        eprintln!("shepard-hook: trace emit failed: {e}");
    }
}

fn post_json(url: &str, payload: &serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
    reqwest::blocking::Client::new()
        .post(url)
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(payload)?)
        .send()?;
    Ok(())
}
