//! Conversion between ACP content blocks and opencode prompt parts.
//!
//! From reference/packages/opencode/src/acp/content.ts.

use url::Url;

use crate::types::{
    Annotations, BlobResourceContents, ContentBlock, ContentChunk, EmbeddedResourceResource, Role,
    TextContent, TextResourceContents,
};

/// A replay part reconstructed from opencode session messages.
#[derive(Debug, Clone)]
pub enum ReplayPart {
    Text {
        text: String,
        synthetic: Option<bool>,
        ignored: Option<bool>,
    },
    File {
        url: String,
        mime: String,
        filename: Option<String>,
    },
    Reasoning {
        text: String,
    },
}

/// `promptContentToParts` from reference/packages/opencode/src/acp/content.ts.
pub fn prompt_content_to_parts(content: &[ContentBlock]) -> Vec<crate::sdk::PromptPart> {
    content.iter().flat_map(content_block_to_parts).collect()
}

/// `contentBlockToParts` from reference/packages/opencode/src/acp/content.ts.
pub fn content_block_to_parts(block: &ContentBlock) -> Vec<crate::sdk::PromptPart> {
    match block {
        ContentBlock::Text(text) => {
            let audience = text
                .annotations
                .as_ref()
                .and_then(|a| a.audience.as_deref());
            let (synthetic, ignored) = audience_flags(audience);
            let mut part = crate::sdk::PromptPart::Text {
                text: text.text.clone(),
                synthetic: None,
                ignored: None,
            };
            if let crate::sdk::PromptPart::Text {
                synthetic: s,
                ignored: i,
                text: _,
            } = &mut part
            {
                *s = synthetic;
                *i = ignored;
            }
            vec![part]
        }
        ContentBlock::Image(image) => {
            if let Some(data) = &image.data {
                return vec![crate::sdk::PromptPart::File {
                    url: format!(
                        "data:{};base64,{}",
                        image.mime_type.clone().unwrap_or_default(),
                        data
                    ),
                    filename: Some(
                        filename_from_uri(image.uri.as_deref()).unwrap_or_else(|| "image".into()),
                    ),
                    mime: image.mime_type.clone().unwrap_or_default(),
                }];
            }
            let uri = match &image.uri {
                Some(uri) if uri.starts_with("data:") => uri,
                Some(uri) if uri.starts_with("http://") || uri.starts_with("https://") => uri,
                _ => return Vec::new(),
            };
            vec![crate::sdk::PromptPart::File {
                url: uri.clone(),
                filename: Some(filename_from_uri(Some(uri)).unwrap_or_else(|| "image".into())),
                mime: image.mime_type.clone().unwrap_or_default(),
            }]
        }
        ContentBlock::ResourceLink(link) => vec![resource_link_to_part(link)],
        ContentBlock::Resource(resource) => resource_to_parts(resource),
    }
}

/// `partsToContentChunks` from reference/packages/opencode/src/acp/content.ts.
pub fn parts_to_content_chunks(parts: &[ReplayPart]) -> Vec<ContentChunk> {
    parts.iter().flat_map(part_to_content_chunks).collect()
}

/// `partToContentChunks` from reference/packages/opencode/src/acp/content.ts.
pub fn part_to_content_chunks(part: &ReplayPart) -> Vec<ContentChunk> {
    match part {
        ReplayPart::Text {
            text,
            synthetic,
            ignored,
        } => {
            if text.is_empty() {
                return Vec::new();
            }
            vec![ContentChunk {
                message_id: None,
                content: ContentBlock::Text(TextContent {
                    text: text.clone(),
                    annotations: part_audience(*synthetic, *ignored),
                }),
            }]
        }
        ReplayPart::File {
            url,
            mime,
            filename,
        } => file_part_to_content_chunks(url, mime, filename.as_deref()),
        ReplayPart::Reasoning { text } => {
            if text.is_empty() {
                return Vec::new();
            }
            vec![ContentChunk {
                message_id: None,
                content: ContentBlock::Text(TextContent {
                    text: text.clone(),
                    annotations: None,
                }),
            }]
        }
    }
}

/// `resourceLinkToPart` from reference/packages/opencode/src/acp/content.ts.
fn resource_link_to_part(link: &crate::types::ResourceLink) -> crate::sdk::PromptPart {
    let mime = link.mime_type.as_deref().unwrap_or("text/plain");
    let name = link.name.as_str();
    uri_to_file_part(&link.uri, mime, Some(name))
}

/// `resourceToParts` — the `resource` branch of `contentBlockToParts`.
fn resource_to_parts(resource: &crate::types::EmbeddedResource) -> Vec<crate::sdk::PromptPart> {
    match &resource.resource {
        EmbeddedResourceResource::Text(text) => match Url::parse(&text.uri) {
            Ok(parsed) if parsed.scheme() == "file" => {
                let line = line_from_hash(parsed.fragment());
                let filepath =
                    file_url_to_path(&parsed).unwrap_or_else(|| percent_decode_path(&text.uri));
                let filepath = filepath.replace('\\', "/");
                vec![crate::sdk::PromptPart::Text {
                    text: format!(
                        "[{filepath}{}]\n{}",
                        line.map(|l| format!(":{l}")).unwrap_or_default(),
                        text.text
                    ),
                    synthetic: None,
                    ignored: None,
                }]
            }
            _ => vec![crate::sdk::PromptPart::Text {
                text: format!("[{}]\n{}", text.uri, text.text),
                synthetic: None,
                ignored: None,
            }],
        },
        EmbeddedResourceResource::Blob(blob) => {
            if let Some(mime) = &blob.mime_type {
                let url = if blob.uri.starts_with("data:") {
                    blob.uri.clone()
                } else {
                    format!("data:{mime};base64,{}", blob.blob)
                };
                vec![crate::sdk::PromptPart::File {
                    url,
                    filename: Some(
                        filename_from_uri(Some(&blob.uri)).unwrap_or_else(|| "file".into()),
                    ),
                    mime: mime.clone(),
                }]
            } else {
                Vec::new()
            }
        }
    }
}

/// `filePartToContentChunks` from reference/packages/opencode/src/acp/content.ts.
fn file_part_to_content_chunks(url: &str, mime: &str, filename: Option<&str>) -> Vec<ContentChunk> {
    if url.starts_with("file://") {
        return vec![ContentChunk {
            message_id: None,
            content: ContentBlock::ResourceLink(crate::types::ResourceLink {
                uri: url.to_string(),
                name: filename.unwrap_or("file").to_string(),
                mime_type: Some(mime.to_string()),
                annotations: None,
            }),
        }];
    }
    if !url.starts_with("data:") {
        return Vec::new();
    }
    let Some(data) = decode_data_url(url) else {
        return Vec::new();
    };
    if data.mime.starts_with("image/") {
        return vec![ContentChunk {
            message_id: None,
            content: ContentBlock::Image(crate::types::ImageContent {
                mime_type: Some(data.mime),
                data: Some(data.base64),
                uri: Some(path_to_file_url(filename.unwrap_or("image"))),
                annotations: None,
            }),
        }];
    }
    vec![ContentChunk {
        message_id: None,
        content: ContentBlock::Resource(crate::types::EmbeddedResource {
            resource: if data.mime.starts_with("text/") || data.mime == "application/json" {
                EmbeddedResourceResource::Text(TextResourceContents {
                    uri: path_to_file_url(filename.unwrap_or("file")),
                    mime_type: Some(data.mime),
                    text: base64_decode(&data.base64),
                })
            } else {
                EmbeddedResourceResource::Blob(BlobResourceContents {
                    uri: path_to_file_url(filename.unwrap_or("file")),
                    mime_type: Some(data.mime),
                    blob: data.base64,
                })
            },
            annotations: None,
        }),
    }]
}

/// `uriToFilePart` from reference/packages/opencode/src/acp/content.ts.
fn uri_to_file_part(uri: &str, mime: &str, filename: Option<&str>) -> crate::sdk::PromptPart {
    if uri.starts_with("file://") {
        return crate::sdk::PromptPart::File {
            url: uri.to_string(),
            filename: Some(
                filename
                    .map(str::to_string)
                    .or_else(|| filename_from_uri(Some(uri)))
                    .unwrap_or_else(|| "file".into()),
            ),
            mime: mime.to_string(),
        };
    }
    if uri.starts_with("zed://") {
        if let Ok(parsed) = Url::parse(uri) {
            if let Some(pathname) = parsed
                .query_pairs()
                .find(|(key, _)| key == "path")
                .map(|(_, value)| value.into_owned())
            {
                let name = filename
                    .map(str::to_string)
                    .unwrap_or_else(|| basename(&pathname).unwrap_or_else(|| "file".into()));
                return crate::sdk::PromptPart::File {
                    url: path_to_file_url(&pathname),
                    filename: Some(name),
                    mime: mime.to_string(),
                };
            }
        }
    }
    crate::sdk::PromptPart::Text {
        text: uri.to_string(),
        synthetic: None,
        ignored: None,
    }
}

/// `decodeDataUrl` from reference/packages/opencode/src/acp/content.ts. The
/// reference regex is `/^data:([^;]+);base64,(.*)$/`.
fn decode_data_url(url: &str) -> Option<DecodedDataUrl> {
    let rest = url.strip_prefix("data:")?;
    let (mime, after_mime) = rest.split_once(';')?;
    let base64 = after_mime.strip_prefix("base64,")?.to_string();
    Some(DecodedDataUrl {
        mime: mime.to_string(),
        base64,
    })
}

struct DecodedDataUrl {
    mime: String,
    base64: String,
}

/// `audienceFlags` from reference/packages/opencode/src/acp/content.ts.
fn audience_flags(audience: Option<&[Role]>) -> (Option<bool>, Option<bool>) {
    match audience {
        Some([Role::Assistant]) => (Some(true), None),
        Some([Role::User]) => (None, Some(true)),
        _ => (None, None),
    }
}

/// `partAudience` from reference/packages/opencode/src/acp/content.ts.
fn part_audience(synthetic: Option<bool>, ignored: Option<bool>) -> Option<Annotations> {
    let audience = if synthetic == Some(true) {
        Some(vec![Role::Assistant])
    } else if ignored == Some(true) {
        Some(vec![Role::User])
    } else {
        return None;
    };
    Some(Annotations { audience })
}

/// `filenameFromUri` from reference/packages/opencode/src/acp/content.ts.
fn filename_from_uri(uri: Option<&str>) -> Option<String> {
    let uri = uri?;
    if uri.starts_with("data:") {
        return None;
    }
    match Url::parse(uri) {
        Ok(parsed) => basename(parsed.path()),
        Err(_) => basename(uri),
    }
}

fn basename(path: &str) -> Option<String> {
    let name = std::path::Path::new(path)
        .file_name()?
        .to_string_lossy()
        .into_owned();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// `fileURLToPath`-equivalent using the `url` crate.
fn file_url_to_path(parsed: &Url) -> Option<String> {
    parsed
        .to_file_path()
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
}

/// `decodeURIComponent(parsed.pathname)` fallback from the reference.
fn percent_decode_path(uri: &str) -> String {
    Url::parse(uri)
        .ok()
        .map(|parsed| parsed.path().to_string())
        .unwrap_or_else(|| uri.to_string())
}

/// `^#L(\d+)` hash extraction from the reference.
fn line_from_hash(fragment: Option<&str>) -> Option<String> {
    let fragment = fragment?;
    let digits = fragment.strip_prefix("L")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(digits.to_string())
}

/// `pathToFileURL(...).href` from the reference.
fn path_to_file_url(path: &str) -> String {
    let path = std::path::Path::new(path);
    if path.is_absolute() {
        Url::from_file_path(path)
            .map(|url| url.to_string())
            .unwrap_or_else(|_| format!("file://{}", path.display()))
    } else {
        // Node's pathToFileURL resolves relative paths against cwd.
        let absolute = std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf());
        Url::from_file_path(&absolute)
            .map(|url| url.to_string())
            .unwrap_or_else(|_| format!("file://{}", absolute.display()))
    }
}

/// `Buffer.from(base64, "base64").toString("utf8")` from the reference.
fn base64_decode(input: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(input)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_content_to_text_part() {
        let block = ContentBlock::Text(TextContent {
            text: "hello".into(),
            annotations: None,
        });
        let parts = content_block_to_parts(&block);
        assert_eq!(
            serde_json::to_value(&parts[0]).unwrap(),
            serde_json::json!({ "type": "text", "text": "hello" })
        );
    }

    #[test]
    fn text_with_synthetic_audience() {
        let block = ContentBlock::Text(TextContent {
            text: "hi".into(),
            annotations: Some(Annotations {
                audience: Some(vec![Role::Assistant]),
            }),
        });
        let parts = content_block_to_parts(&block);
        assert_eq!(
            serde_json::to_value(&parts[0]).unwrap(),
            serde_json::json!({ "type": "text", "text": "hi", "synthetic": true })
        );
    }

    #[test]
    fn file_url_part_to_resource_link_chunk() {
        let part = ReplayPart::File {
            url: "file:///tmp/a.txt".into(),
            mime: "text/plain".into(),
            filename: Some("a.txt".into()),
        };
        let chunks = part_to_content_chunks(&part);
        assert_eq!(
            serde_json::to_value(&chunks[0]).unwrap(),
            serde_json::json!({
                "content": {
                    "type": "resource_link",
                    "uri": "file:///tmp/a.txt",
                    "name": "a.txt",
                    "mimeType": "text/plain"
                }
            })
        );
    }

    #[test]
    fn data_url_image_to_image_chunk() {
        let part = ReplayPart::File {
            url: "data:image/png;base64,AAAA".into(),
            mime: "image/png".into(),
            filename: Some("/tmp/img".into()),
        };
        let chunks = part_to_content_chunks(&part);
        assert_eq!(
            serde_json::to_value(&chunks[0]).unwrap(),
            serde_json::json!({
                "content": {
                    "type": "image",
                    "mimeType": "image/png",
                    "data": "AAAA",
                    "uri": "file:///tmp/img"
                }
            })
        );
    }

    #[test]
    fn resource_link_file_to_file_part() {
        let link = crate::types::ResourceLink {
            uri: "file:///tmp/a.txt".into(),
            name: "a.txt".into(),
            mime_type: Some("text/plain".into()),
            annotations: None,
        };
        let parts = content_block_to_parts(&ContentBlock::ResourceLink(link));
        assert_eq!(
            serde_json::to_value(&parts[0]).unwrap(),
            serde_json::json!({
                "type": "file",
                "url": "file:///tmp/a.txt",
                "filename": "a.txt",
                "mime": "text/plain"
            })
        );
    }
}
