use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::otlp;

static COLLECTOR_BASE: LazyLock<String> = LazyLock::new(|| {
    std::env::var("OTEL_HTTP_URL").unwrap_or_else(|_| "http://localhost:4318".to_string())
});

static HTTP_CLIENT: LazyLock<reqwest::blocking::Client> =
    LazyLock::new(reqwest::blocking::Client::new);

/// Fire-and-forget metric POST. Errors logged to stderr, never propagated.
pub fn metric(name: &str, value: f64, labels: &HashMap<String, String>) {
    let now_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos().to_string())
        .unwrap_or_else(|_| "0".to_string());

    let payload = otlp::build_sum_metric(name, value, labels, &now_nanos);
    let url = format!("{}/v1/metrics", *COLLECTOR_BASE);

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
    let url = format!("{}/v1/traces", *COLLECTOR_BASE);

    if let Err(e) = post_json(&url, &payload) {
        eprintln!("shepard-hook: trace emit failed: {e}");
    }
}

fn post_json(url: &str, payload: &serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
    HTTP_CLIENT
        .post(url)
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(payload)?)
        .send()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collector_base_returns_string() {
        let base = &*COLLECTOR_BASE;
        assert!(base.starts_with("http"));
    }

    #[test]
    fn traces_skips_empty_spans() {
        traces("test-service", &[]);
    }

    #[test]
    fn metric_does_not_panic_on_connection_refused() {
        let payload = crate::otlp::build_sum_metric("test", 1.0, &HashMap::new(), "0");
        let result = post_json("http://127.0.0.1:1/v1/metrics", &payload);
        assert!(result.is_err());
    }

    #[test]
    fn traces_does_not_panic_on_connection_refused() {
        let spans = vec![serde_json::json!({
            "trace_id": "abc123",
            "span_id": "0000000000000001",
            "parent_span_id": "",
            "name": "test.span",
            "start_ns": "1000",
            "end_ns": "2000",
            "status": 0,
            "attributes": {}
        })];
        let payload = crate::otlp::build_trace_export("test", &spans);
        let result = post_json("http://127.0.0.1:1/v1/traces", &payload);
        assert!(result.is_err());
    }
}
