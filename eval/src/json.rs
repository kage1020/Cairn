//! JSON written so that equal values print equal, whatever order their keys came in.

use serde_json::Value;

/// `value` as JSON with every object's keys sorted.
pub(crate) fn canonical(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let fields: Vec<String> = keys
                .into_iter()
                .map(|key| format!("{}:{}", Value::from(key.as_str()), canonical(&map[key])))
                .collect();
            format!("{{{}}}", fields.join(","))
        }
        Value::Array(items) => {
            let items: Vec<String> = items.iter().map(canonical).collect();
            format!("[{}]", items.join(","))
        }
        other => other.to_string(),
    }
}
