use serde_json::{json, Value};
use std::collections::HashMap;

/// Build an OTLP ExportMetricsServiceRequest with a single Sum metric (DELTA).
pub fn build_sum_metric(
    name: &str,
    value: f64,
    labels: &HashMap<String, String>,
    time_unix_nano: &str,
) -> Value {
    let attrs: Vec<Value> = labels
        .iter()
        .map(|(k, v)| {
            json!({
                "key": k,
                "value": { "stringValue": v }
            })
        })
        .collect();

    json!({
        "resourceMetrics": [{
            "resource": {
                "attributes": [{
                    "key": "service.name",
                    "value": { "stringValue": "shepherd-hooks" }
                }]
            },
            "scopeMetrics": [{
                "scope": { "name": "shepherd-hooks" },
                "metrics": [{
                    "name": name,
                    "sum": {
                        "dataPoints": [{
                            "asDouble": value,
                            "timeUnixNano": time_unix_nano,
                            "attributes": attrs
                        }],
                        "aggregationTemporality": 1,
                        "isMonotonic": true
                    }
                }]
            }]
        }]
    })
}

/// Build an OTLP ExportTraceServiceRequest from parsed span JSON values.
///
/// Input spans have the shape:
/// ```json
/// { "trace_id", "span_id", "parent_span_id", "name", "start_ns", "end_ns",
///   "status": 0|2, "attributes": {...} }
/// ```
pub fn build_trace_export(service_name: &str, spans: &[Value]) -> Value {
    let otlp_spans: Vec<Value> = spans
        .iter()
        .map(|s| {
            let attrs: Vec<Value> = s["attributes"]
                .as_object()
                .map(|obj| {
                    obj.iter()
                        .map(|(k, v)| {
                            let val_str = v.as_str().unwrap_or("");
                            let value = if val_str.chars().all(|c| c.is_ascii_digit())
                                && !val_str.is_empty()
                            {
                                json!({ "intValue": val_str })
                            } else {
                                json!({ "stringValue": val_str })
                            };
                            json!({ "key": k, "value": value })
                        })
                        .collect()
                })
                .unwrap_or_default();

            json!({
                "traceId": s["trace_id"],
                "spanId": s["span_id"],
                "parentSpanId": s["parent_span_id"],
                "name": s["name"],
                "kind": 1,
                "startTimeUnixNano": s["start_ns"],
                "endTimeUnixNano": s["end_ns"],
                "attributes": attrs,
                "status": { "code": s["status"] }
            })
        })
        .collect();

    json!({
        "resourceSpans": [{
            "resource": {
                "attributes": [{
                    "key": "service.name",
                    "value": { "stringValue": service_name }
                }]
            },
            "scopeSpans": [{
                "scope": { "name": "shepherd-session-parser", "version": "0.1.0" },
                "spans": otlp_spans
            }]
        }]
    })
}
