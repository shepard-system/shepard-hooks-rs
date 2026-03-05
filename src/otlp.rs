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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_sum_metric_has_correct_structure() {
        let mut labels = HashMap::new();
        labels.insert("source".into(), "claude-code".into());
        labels.insert("tool".into(), "Read".into());

        let result = build_sum_metric("tool_calls", 1.0, &labels, "1234567890");

        // Top-level structure
        let rm = &result["resourceMetrics"][0];
        let svc = &rm["resource"]["attributes"][0];
        assert_eq!(svc["key"], "service.name");
        assert_eq!(svc["value"]["stringValue"], "shepherd-hooks");

        // Metric name and value
        let metric = &rm["scopeMetrics"][0]["metrics"][0];
        assert_eq!(metric["name"], "tool_calls");
        assert_eq!(metric["sum"]["dataPoints"][0]["asDouble"], 1.0);
        assert_eq!(metric["sum"]["dataPoints"][0]["timeUnixNano"], "1234567890");
        assert_eq!(metric["sum"]["aggregationTemporality"], 1);
        assert_eq!(metric["sum"]["isMonotonic"], true);

        // Labels present in dataPoint attributes
        let dp_attrs = metric["sum"]["dataPoints"][0]["attributes"]
            .as_array()
            .unwrap();
        assert_eq!(dp_attrs.len(), 2);
    }

    #[test]
    fn build_trace_export_wraps_spans() {
        let spans = vec![json!({
            "trace_id": "abc123",
            "span_id": "0000000000000001",
            "parent_span_id": "",
            "name": "test.session",
            "start_ns": "1000",
            "end_ns": "2000",
            "status": 0,
            "attributes": { "provider": "test", "tokens.input": "100" }
        })];

        let result = build_trace_export("test-service", &spans);

        // Top-level structure
        let rs = &result["resourceSpans"][0];
        let svc = &rs["resource"]["attributes"][0];
        assert_eq!(svc["key"], "service.name");
        assert_eq!(svc["value"]["stringValue"], "test-service");

        // Scope
        let scope = &rs["scopeSpans"][0]["scope"];
        assert_eq!(scope["name"], "shepherd-session-parser");
        assert_eq!(scope["version"], "0.1.0");

        // Span mapping
        let otlp_span = &rs["scopeSpans"][0]["spans"][0];
        assert_eq!(otlp_span["traceId"], "abc123");
        assert_eq!(otlp_span["spanId"], "0000000000000001");
        assert_eq!(otlp_span["name"], "test.session");
        assert_eq!(otlp_span["kind"], 1);
        assert_eq!(otlp_span["status"]["code"], 0);

        // Numeric attribute uses intValue, string uses stringValue
        let attrs = otlp_span["attributes"].as_array().unwrap();
        let provider_attr = attrs.iter().find(|a| a["key"] == "provider").unwrap();
        assert!(provider_attr["value"]["stringValue"].is_string());
        let tokens_attr = attrs.iter().find(|a| a["key"] == "tokens.input").unwrap();
        assert!(tokens_attr["value"]["intValue"].is_string());
    }
}
