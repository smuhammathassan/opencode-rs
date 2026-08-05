/// From reference/packages/tui/src/util/record.ts
pub fn is_record(value: &serde_json::Value) -> bool {
    matches!(value, serde_json::Value::Object(_))
}

#[cfg(test)]
mod tests {
    use super::is_record;
    use serde_json::json;

    #[test]
    fn objects_are_records() {
        assert!(is_record(&json!({ "a": 1 })));
    }

    #[test]
    fn non_objects_are_not_records() {
        assert!(!is_record(&json!(null)));
        assert!(!is_record(&json!(42)));
        assert!(!is_record(&json!("str")));
        assert!(!is_record(&json!([1, 2, 3])));
        assert!(!is_record(&json!(true)));
    }
}
