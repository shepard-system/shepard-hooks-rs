use std::collections::HashMap;
use std::error::Error;

use crate::emit;

pub fn run(name: &str, value: f64, labels_json: &str) -> Result<(), Box<dyn Error>> {
    let labels: HashMap<String, String> = serde_json::from_str(labels_json)?;
    emit::metric(name, value, &labels);
    Ok(())
}
