//! JSON written so that equal values print equal, whatever order their keys came in.

use serde_json::Value;

/// `value` as JSON with every object's keys sorted.
pub(crate) fn canonical(mut value: Value) -> String {
    value.sort_all_objects();
    value.to_string()
}
