//! Prompt "parts" — attachments (files, agents, pasted text) embedded in the
//! prompt as styled virtual-text markers.
//!
//! In the reference, `TextareaRenderable` extmarks back each part
//! (`reference/packages/tui/src/component/prompt/index.tsx`,
//! `pasteText`/`pasteAttachment`/`restoreExtmarksFromParts`). The buffer text
//! contains the marker (`[Image 1]`, `@agent`, `[Pasted ~N lines]`); on submit
//! text parts are expanded inline
//! (`reference/packages/tui/src/prompt/part.ts`, `expandTrackedPastedText`)
//! while file/agent markers are stripped and their parts attached separately.

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartKind {
    File,
    Agent,
    PastedText,
}

#[derive(Debug, Clone)]
pub struct PartRange {
    pub start: usize,
    pub end: usize,
    pub kind: PartKind,
    pub part_index: usize,
}

pub fn source_text(part: &Value) -> Option<(usize, usize, String)> {
    let map = part.as_object()?;
    let source = map.get("source")?.as_object()?;
    // File/text parts nest the marker under `source.text`; agent parts keep it
    // directly on the source object.
    if let Some(text) = source.get("text").and_then(Value::as_object) {
        let value = text.get("value")?.as_str()?.to_string();
        let start = text.get("start")?.as_u64()? as usize;
        let end = text.get("end")?.as_u64()? as usize;
        return Some((start, end, value));
    }
    let value = source.get("value")?.as_str()?.to_string();
    let start = source.get("start")?.as_u64()? as usize;
    let end = source.get("end")?.as_u64()? as usize;
    Some((start, end, value))
}

pub fn part_kind(part: &Value) -> Option<PartKind> {
    let type_ = part.get("type")?.as_str()?;
    match type_ {
        "file" => Some(PartKind::File),
        "agent" => Some(PartKind::Agent),
        "text" => Some(PartKind::PastedText),
        _ => None,
    }
}

/// Read the marker ranges referenced by `parts`.
pub fn part_ranges(parts: &[Value]) -> Vec<PartRange> {
    parts
        .iter()
        .enumerate()
        .filter_map(|(index, part)| {
            let (start, end, _) = source_text(part)?;
            let kind = part_kind(part)?;
            Some(PartRange {
                start,
                end,
                kind,
                part_index: index,
            })
        })
        .collect()
}

/// Strip markers (file/agent/pasted-text ranges) from buffer text.
pub fn plain_text(buffer: &str, parts: &[Value]) -> String {
    let mut ranges = part_ranges(parts);
    ranges.sort_by_key(|range| std::cmp::Reverse(range.start));
    let mut result = buffer.to_string();
    for range in ranges {
        if range.start <= result.chars().count() && range.end <= result.chars().count() {
            result = replace_char_range(&result, range.start, range.end, "");
        }
    }
    result
}

/// Replace text-part markers with their real content; strip file/agent markers.
/// From reference/packages/tui/src/prompt/part.ts (`expandTrackedPastedText`)
pub fn expand_text_parts(buffer: &str, parts: &[Value]) -> String {
    let mut edits: Vec<(usize, usize, String)> = parts
        .iter()
        .filter_map(|part| {
            if part_kind(part) != Some(PartKind::PastedText) {
                return None;
            }
            let (start, end, _) = source_text(part)?;
            let text = part.get("text")?.as_str()?.to_string();
            Some((start, end, text))
        })
        .collect();
    // Also strip file/agent markers.
    for part in parts.iter() {
        let Some((start, end, _)) = source_text(part) else {
            continue;
        };
        let Some(kind) = part_kind(part) else {
            continue;
        };
        if kind != PartKind::PastedText && !edits.iter().any(|(s, e, _)| *s == start && *e == end) {
            edits.push((start, end, String::new()));
        }
    }
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.1));
    let mut result = buffer.to_string();
    for (start, end, replacement) in edits {
        result = replace_char_range(&result, start, end, &replacement);
    }
    result
}

/// Expand `[Pasted …]`-style placeholders when copying text out of the prompt.
/// From reference/packages/tui/src/prompt/part.ts (`expandPastedTextPlaceholders`)
pub fn expand_pasted_text_placeholders(text: &str, parts: &[Value]) -> String {
    let mut result = text.to_string();
    for part in parts {
        let Some((start, end, value)) = source_text(part) else {
            continue;
        };
        if part_kind(part) != Some(PartKind::PastedText) {
            continue;
        }
        let Some(replacement) = part.get("text").and_then(Value::as_str) else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        if let Some((pos, _end)) = find_substring(&result, &value) {
            result = replace_char_range(&result, pos, pos + value.chars().count(), replacement);
        }
        let _ = (start, end);
    }
    result
}

/// After a buffer edit, re-locate each part's marker and drop parts whose
/// marker was deleted. When a marker appears multiple times, the occurrence
/// nearest the previous position is chosen.
pub fn sync_part_ranges(text: &str, parts: &mut Vec<Value>) {
    let mut kept: Vec<Value> = Vec::with_capacity(parts.len());
    for part in parts.drain(..) {
        let Some((start, _, value)) = source_text(&part) else {
            kept.push(part);
            continue;
        };
        if value.is_empty() {
            kept.push(part);
            continue;
        }
        match find_nearest_occurrence(text, &value, start) {
            Some((found_start, found_end)) => {
                let mut part = part;
                if let Some(source) = part.get_mut("source").and_then(Value::as_object_mut) {
                    if let Some(text) = source.get_mut("text").and_then(Value::as_object_mut) {
                        text.insert("start".into(), Value::from(found_start));
                        text.insert("end".into(), Value::from(found_end));
                    } else {
                        source.insert("start".into(), Value::from(found_start));
                        source.insert("end".into(), Value::from(found_end));
                    }
                }
                kept.push(part);
            }
            None => {
                // Marker text deleted — drop the part.
            }
        }
    }
    *parts = kept;
}

/// Strip `id`/`messageID`/`sessionID` from parts before sending.
/// From reference/packages/tui/src/prompt/part.ts (`stripPromptPartIDs`)
pub fn strip_prompt_part_ids(parts: &[Value]) -> Vec<Value> {
    parts
        .iter()
        .map(|part| {
            let mut part = part.clone();
            if let Some(map) = part.as_object_mut() {
                map.remove("id");
                map.remove("messageID");
                map.remove("sessionID");
            }
            part
        })
        .collect()
}

fn replace_char_range(text: &str, start: usize, end: usize, replacement: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    if start > chars.len() || end > chars.len() || start > end {
        return text.to_string();
    }
    let mut out: String = chars[..start].iter().collect();
    out.push_str(replacement);
    out.extend(chars[end..].iter());
    out
}

fn find_substring(text: &str, needle: &str) -> Option<(usize, usize)> {
    find_substring_from(text, needle, 0)
}

/// Find the occurrence of `needle` closest to `near` (preferring earlier ties).
fn find_nearest_occurrence(text: &str, needle: &str, near: usize) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize)> = None;
    let mut search_from = 0usize;
    while let Some((start, end)) = find_substring_from(text, needle, search_from) {
        let distance = (start as i64 - near as i64).unsigned_abs();
        let better = match best {
            None => true,
            Some((bs, _)) => distance < (bs as i64 - near as i64).unsigned_abs(),
        };
        if better {
            best = Some((start, end));
        }
        search_from = end;
    }
    best
}

fn find_substring_from(text: &str, needle: &str, from: usize) -> Option<(usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    let needle: Vec<char> = needle.chars().collect();
    if needle.is_empty() || needle.len() > chars.len() {
        return None;
    }
    let start = from.min(chars.len());
    for i in start..=chars.len() - needle.len() {
        if chars[i..i + needle.len()] == needle[..] {
            return Some((i, i + needle.len()));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn file_part(filename: &str, marker: &str, start: usize, end: usize) -> Value {
        json!({
            "type": "file",
            "mime": "image/png",
            "filename": filename,
            "url": "data:image/png;base64,xxx",
            "source": { "type": "file", "path": filename, "text": { "value": marker, "start": start, "end": end } }
        })
    }

    fn text_part(content: &str, marker: &str, start: usize, end: usize) -> Value {
        json!({
            "type": "text",
            "text": content,
            "source": { "text": { "value": marker, "start": start, "end": end } }
        })
    }

    fn agent_part(name: &str, start: usize, end: usize) -> Value {
        json!({
            "type": "agent",
            "name": name,
            "source": { "value": format!("@{name}"), "start": start, "end": end }
        })
    }

    #[test]
    fn plain_text_strips_markers() {
        let parts = vec![
            file_part("a.png", "[Image 1]", 0, 9),
            agent_part("build", 10, 16),
        ];
        let buffer = "[Image 1] @build hello";
        assert_eq!(plain_text(buffer, &parts), "  hello");
    }

    #[test]
    fn expand_text_parts_replaces_content() {
        let parts = vec![text_part("real pasted body", "[Pasted ~10 lines]", 0, 18)];
        let buffer = "[Pasted ~10 lines] rest";
        assert_eq!(expand_text_parts(buffer, &parts), "real pasted body rest");
    }

    #[test]
    fn expand_text_parts_strips_file_markers() {
        let parts = vec![file_part("a.png", "[Image 1]", 0, 9)];
        let buffer = "[Image 1] hello";
        assert_eq!(expand_text_parts(buffer, &parts), " hello");
    }

    #[test]
    fn sync_drops_deleted_markers() {
        let mut parts = vec![
            file_part("a.png", "[Image 1]", 0, 9),
            agent_part("build", 10, 16),
        ];
        // Delete the file marker from the buffer text.
        let new_text = "@build hello";
        sync_part_ranges(new_text, &mut parts);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["type"], "agent");
        // The agent marker moved to the front.
        let (start, end, _) = source_text(&parts[0]).unwrap();
        assert_eq!((start, end), (0, 6));
    }

    #[test]
    fn sync_updates_shifted_ranges() {
        let mut parts = vec![file_part("a.png", "[Image 1]", 5, 14)];
        let new_text = "prefix [Image 1] suffix";
        sync_part_ranges(new_text, &mut parts);
        let (start, end, _) = source_text(&parts[0]).unwrap();
        assert_eq!((start, end), (7, 16));
    }

    #[test]
    fn strip_ids_removes_fields() {
        let mut parts = vec![file_part("a.png", "[Image 1]", 0, 9)];
        parts[0]["id"] = json!("pt_1");
        parts[0]["messageID"] = json!("msg_1");
        parts[0]["sessionID"] = json!("ses_1");
        let stripped = strip_prompt_part_ids(&parts);
        assert!(stripped[0].get("id").is_none());
        assert!(stripped[0].get("messageID").is_none());
        assert!(stripped[0].get("sessionID").is_none());
        assert_eq!(stripped[0]["filename"], "a.png");
    }

    #[test]
    fn placeholder_expansion_on_copy() {
        let parts = vec![text_part("real body", "[Pasted ~3 lines]", 0, 17)];
        assert_eq!(
            expand_pasted_text_placeholders("[Pasted ~3 lines] tail", &parts),
            "real body tail"
        );
    }

    #[test]
    fn duplicate_markers_locate_distinctly() {
        let parts = vec![
            file_part("a.png", "[Image 1]", 0, 9),
            file_part("b.png", "[Image 1]", 10, 19),
        ];
        let buffer = "[Image 1] [Image 1]";
        assert_eq!(plain_text(buffer, &parts), " ");
        let mut parts = parts;
        sync_part_ranges(buffer, &mut parts);
        assert_eq!(parts.len(), 2);
        let (s1, e1, _) = source_text(&parts[0]).unwrap();
        let (s2, e2, _) = source_text(&parts[1]).unwrap();
        assert_eq!((s1, e1), (0, 9));
        assert_eq!((s2, e2), (10, 19));
    }
}
