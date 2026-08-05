//! Bedrock media lowering (image + document blocks).
//! From reference/packages/llm/src/protocols/utils/bedrock-media.ts

use serde_json::{Map, Value};
use std::collections::HashSet;

use crate::schema::messages::{MediaData, MediaPart};
use crate::schema::LlmError;
use crate::shared;

pub const IMAGE_FORMATS: [(&str, &str); 5] = [
    ("image/png", "png"),
    ("image/jpeg", "jpeg"),
    ("image/jpg", "jpeg"),
    ("image/gif", "gif"),
    ("image/webp", "webp"),
];

pub const DOCUMENT_FORMATS: [(&str, &str); 9] = [
    ("application/pdf", "pdf"),
    ("text/csv", "csv"),
    ("application/msword", "doc"),
    (
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "docx",
    ),
    ("application/vnd.ms-excel", "xls"),
    (
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "xlsx",
    ),
    ("text/html", "html"),
    ("text/plain", "txt"),
    ("text/markdown", "md"),
];

/// `BedrockMedia.lower(part)`.
/// From reference/packages/llm/src/protocols/utils/bedrock-media.ts (`lower`)
pub fn lower(part: &MediaPart) -> Result<Value, LlmError> {
    let mime = part.media_type.to_lowercase();
    let image_format = IMAGE_FORMATS
        .iter()
        .find(|(mime_type, _)| *mime_type == mime)
        .map(|(_, format)| *format);
    if let Some(format) = image_format {
        let supported: HashSet<String> = IMAGE_FORMATS.iter().map(|(m, _)| m.to_string()).collect();
        let media = shared::validate_media("Bedrock Converse", part, &supported)?;
        return Ok(Value::Object(Map::from_iter([(
            "image".to_string(),
            Value::Object(Map::from_iter([
                ("format".to_string(), Value::String(format.to_string())),
                (
                    "source".to_string(),
                    Value::Object(Map::from_iter([(
                        "bytes".to_string(),
                        Value::String(media.base64),
                    )])),
                ),
            ])),
        )])));
    }
    if mime.starts_with("image/") {
        return Err(shared::invalid_request(format!(
            "Bedrock Converse does not support image media type {}",
            part.media_type
        )));
    }
    let document_format = DOCUMENT_FORMATS
        .iter()
        .find(|(mime_type, _)| *mime_type == mime)
        .map(|(_, format)| *format);
    if let Some(format) = document_format {
        let supported: HashSet<String> = DOCUMENT_FORMATS
            .iter()
            .map(|(m, _)| m.to_string())
            .collect();
        let media = shared::validate_media("Bedrock Converse", part, &supported)?;
        return Ok(document_block(part, format, media.base64));
    }
    Err(shared::invalid_request(format!(
        "Bedrock Converse does not support media type {}",
        part.media_type
    )))
}

fn document_block(part: &MediaPart, format: &str, bytes: String) -> Value {
    let name = part
        .filename
        .clone()
        .unwrap_or_else(|| format!("document.{}", format));
    Value::Object(Map::from_iter([(
        "document".to_string(),
        Value::Object(Map::from_iter([
            ("format".to_string(), Value::String(format.to_string())),
            ("name".to_string(), Value::String(name)),
            (
                "source".to_string(),
                Value::Object(Map::from_iter([(
                    "bytes".to_string(),
                    Value::String(bytes),
                )])),
            ),
        ])),
    )]))
}

/// `BedrockMedia` namespace marker.
#[allow(unused)]
pub(crate) fn _marker(_: &MediaData) {}
