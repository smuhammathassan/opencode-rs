// Deep merge with remeda `mergeDeep` semantics.
//
// From reference/packages/opencode/src/config/config.ts (`mergeConfig`).

use serde_json::Value;

/// Recursively merges `source` into `target`, mirroring remeda's `mergeDeep`:
/// object values merge recursively, arrays merge element-wise by index, and
/// every other value (including `null`) is replaced by `source`. Arrays are
/// treated as object-like, so `merge([1,2], [3])` yields `[3, 2]`.
pub fn merge_deep(target: &Value, source: &Value) -> Value {
    match (target, source) {
        (Value::Object(target), Value::Object(source)) => {
            let mut out = target.clone();
            for (key, value) in source {
                let merged = match out.get(key) {
                    Some(existing) => merge_deep(existing, value),
                    None => value.clone(),
                };
                out.insert(key.clone(), merged);
            }
            Value::Object(out)
        }
        (Value::Array(target), Value::Array(source)) => {
            let len = target.len().max(source.len());
            let mut out = Vec::with_capacity(len);
            for i in 0..len {
                out.push(match (target.get(i), source.get(i)) {
                    (Some(target), Some(source)) => merge_deep(target, source),
                    (Some(target), None) => target.clone(),
                    (None, Some(source)) => source.clone(),
                    (None, None) => unreachable!(),
                });
            }
            Value::Array(out)
        }
        _ => source.clone(),
    }
}

/// Concatenates and de-duplicates two string lists while preserving order —
/// the `instructions` special case in `mergeConfigConcatArrays`.
pub fn concat_unique(target: &[String], source: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for item in target.iter().chain(source.iter()) {
        if seen.insert(item.clone()) {
            out.push(item.clone());
        }
    }
    out
}

/// De-duplicates while keeping the *last* occurrence of each item, preserving
/// the relative order of last occurrences — used for plugin origins.
pub fn dedupe_keep_last<I, T, K, F>(items: I, key: F) -> Vec<T>
where
    I: IntoIterator<Item = T>,
    I::IntoIter: DoubleEndedIterator,
    K: Eq + std::hash::Hash,
    F: Fn(&T) -> K,
{
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for item in items.into_iter().rev() {
        if seen.insert(key(&item)) {
            out.push(item);
        }
    }
    out.reverse();
    out
}
