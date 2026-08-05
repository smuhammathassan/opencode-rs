//! Sampling defaults and token caps.
//!
//! From `transform.ts`: `sanitizeSurrogates`, `temperature`, `topP`, `topK`,
//! `maxOutputTokens`.

use crate::provider::Model;

/// Replaces lone surrogate code points with U+FFFD.
///
/// From `sanitizeSurrogates()` in `transform.ts`. Rust strings are valid UTF-8
/// so lone surrogate code points cannot occur; `serde_json` already decodes
/// lone `\uD8xx` escapes to U+FFFD during parsing. This is therefore the
/// identity function and is kept for signature parity.
pub fn sanitize_surrogates(content: &str) -> String {
    content.to_string()
}

pub(crate) fn mime_to_modality(mime: &str) -> Option<&'static str> {
    if mime.starts_with("image/") {
        return Some("image");
    }
    if mime.starts_with("audio/") {
        return Some("audio");
    }
    if mime.starts_with("video/") {
        return Some("video");
    }
    if mime == "application/pdf" {
        return Some("pdf");
    }
    None
}

pub(crate) fn is_kimi_family(model: &Model) -> bool {
    let ids = [&model.provider_id, &model.api.id];
    if ids.iter().any(|id| {
        let value = id.to_lowercase();
        value.contains("kimi") || value.contains("moonshot")
    }) {
        return true;
    }
    let url = model.api.url.to_lowercase();
    [
        "api.kimi.com",
        "api.moonshot.ai",
        "api.moonshot.cn",
        "api.moonshotai.cn",
    ]
    .iter()
    .any(|host| url.contains(host))
}

/// Gemini sampling-default regexes from `transform.ts`.
///
/// The reference uses `Regex.test` over a set of patterns; one uses a negative
/// lookahead (`gemini-3.5-flash(?!-lite)`) which the `regex` crate does not
/// support, so the patterns are matched manually.
fn gemini_sampling_defaults(id: &str) -> bool {
    gemini_2_5(id) || gemini_3_flash_pro(id) || gemini_3_1(id) || gemini_3_5_flash_no_lite(id)
}

fn gemini_2_5(id: &str) -> bool {
    for (idx, _) in id.match_indices("gemini-2") {
        let rest = &id[idx + 8..].as_bytes();
        if rest.len() >= 2 && (rest[0] == b'.' || rest[0] == b'-') && rest[1] == b'5' {
            let tail = &rest[2..];
            if tail.is_empty() || tail[0] == b'.' || tail[0] == b'-' {
                return true;
            }
        }
    }
    false
}

fn gemini_3_flash_pro(id: &str) -> bool {
    for (idx, _) in id.match_indices("gemini-3-") {
        let rest = &id[idx + 9..];
        for family in ["flash", "pro"] {
            if let Some(tail) = rest.strip_prefix(family) {
                if tail.is_empty() || tail.starts_with('.') || tail.starts_with('-') {
                    return true;
                }
            }
        }
    }
    false
}

fn gemini_3_1(id: &str) -> bool {
    for (idx, _) in id.match_indices("gemini-3") {
        let rest = &id[idx + 8..].as_bytes();
        if rest.len() >= 2 && (rest[0] == b'.' || rest[0] == b'-') && rest[1] == b'1' {
            let tail = &rest[2..];
            if tail.is_empty() || tail[0] == b'.' || tail[0] == b'-' {
                return true;
            }
        }
    }
    false
}

fn gemini_3_5_flash_no_lite(id: &str) -> bool {
    for (idx, _) in id.match_indices("gemini-3") {
        let rest = &id[idx + 8..].as_bytes();
        if rest.len() >= 2 && (rest[0] == b'.' || rest[0] == b'-') && rest[1] == b'5' {
            let tail = &rest[2..];
            if let Some(after) = tail.strip_prefix(b"-flash") {
                if after.starts_with(b"-lite") {
                    continue;
                }
                if after.is_empty() || after[0] == b'.' || after[0] == b'-' {
                    return true;
                }
            }
        }
    }
    false
}

/// Recommended `temperature` for a model.
///
/// From `temperature()` in `transform.ts`.
pub fn temperature(model: &Model) -> Option<f64> {
    let id = model.api.id.to_lowercase();
    if id.contains("north-mini-code") {
        return Some(1.0);
    }
    if id.contains("qwen") {
        return Some(0.55);
    }
    if id.contains("claude") {
        return None;
    }
    if id.contains("gemini") {
        return gemini_sampling_defaults(&id).then_some(1.0);
    }
    if id.contains("glm-4.6") || id.contains("glm-4.7") || id.contains("minimax-m2") {
        return Some(1.0);
    }
    if id.contains("kimi-k2") {
        if ["thinking", "k2.", "k2p", "k2-5"]
            .iter()
            .any(|s| id.contains(s))
        {
            return Some(1.0);
        }
        return Some(0.6);
    }
    None
}

/// Recommended `topP` for a model.
///
/// From `topP()` in `transform.ts`.
pub fn top_p(model: &Model) -> Option<f64> {
    let id = model.api.id.to_lowercase();
    if id.contains("qwen") {
        return Some(1.0);
    }
    if id.contains("gemini") {
        return gemini_sampling_defaults(&id).then_some(0.95);
    }
    if ["minimax-m2", "kimi-k2.5", "kimi-k2p5", "kimi-k2-5"]
        .iter()
        .any(|s| id.contains(s))
    {
        return Some(0.95);
    }
    None
}

/// Recommended `topK` for a model.
///
/// From `topK()` in `transform.ts`.
pub fn top_k(model: &Model) -> Option<u64> {
    let id = model.api.id.to_lowercase();
    if id.contains("minimax-m2") {
        if ["m2.", "m25", "m21"].iter().any(|s| id.contains(s)) {
            return Some(40);
        }
        return Some(20);
    }
    if id.contains("gemini") {
        return gemini_sampling_defaults(&id).then_some(64);
    }
    None
}

/// Caps `maxOutputTokens` at `output_token_max`.
///
/// From `maxOutputTokens()` in `transform.ts`.
pub fn max_output_tokens(model: &Model, output_token_max: f64) -> f64 {
    let value = model.limit.output.min(output_token_max);
    if value == 0.0 {
        output_token_max
    } else {
        value
    }
}
