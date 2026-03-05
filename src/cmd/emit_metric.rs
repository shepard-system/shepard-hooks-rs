use std::collections::HashMap;
use std::error::Error;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::otlp;

const COLLECTOR_URL: &str = "http://localhost:4318/v1/metrics";

pub fn run(name: &str, value: f64, labels_json: &str) -> Result<(), Box<dyn Error>> {
    let labels: HashMap<String, String> = serde_json::from_str(labels_json)?;
    let now_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_nanos()
        .to_string();

    let payload = otlp::build_sum_metric(name, value, &labels, &now_nanos);

    reqwest::blocking::Client::new()
        .post(COLLECTOR_URL)
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&payload)?)
        .send()?;

    Ok(())
}
