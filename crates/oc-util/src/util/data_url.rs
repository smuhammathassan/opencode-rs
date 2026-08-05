/// From reference/packages/opencode/src/util/data-url.ts
///
/// Decodes a `data:` URL body. Mirrors `Buffer.from(body, "base64")` (lenient:
/// ignores non-base64 characters and leftover bits) for `;base64` URLs and
/// `decodeURIComponent` for everything else.
use std::fmt;

#[derive(Debug)]
pub enum DecodeError {
    MalformedPercentEncoding,
    InvalidUtf8,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::MalformedPercentEncoding => write!(f, "URI malformed"),
            DecodeError::InvalidUtf8 => write!(f, "malformed UTF-8 data"),
        }
    }
}

impl std::error::Error for DecodeError {}

pub fn decode_data_url(url: &str) -> Result<String, DecodeError> {
    let Some(idx) = url.find(',') else {
        return Ok(String::new());
    };
    let head = &url[..idx];
    let body = &url[idx + 1..];
    if head.contains(";base64") {
        Ok(decode_base64(body))
    } else {
        decode_uri_component(body)
    }
}

fn hex_value(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn decode_uri_component(input: &str) -> Result<String, DecodeError> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err(DecodeError::MalformedPercentEncoding);
            }
            let hi = hex_value(bytes[i + 1]).ok_or(DecodeError::MalformedPercentEncoding)?;
            let lo = hex_value(bytes[i + 2]).ok_or(DecodeError::MalformedPercentEncoding)?;
            out.push(hi * 16 + lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| DecodeError::InvalidUtf8)
}

fn base64_value(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn decode_base64(input: &str) -> String {
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    let mut out = Vec::new();
    for &c in input.as_bytes() {
        let Some(value) = base64_value(c) else {
            continue;
        };
        acc = (acc << 6) | value as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_comma_returns_empty() {
        assert_eq!(decode_data_url("no comma here").unwrap(), "");
    }

    #[test]
    fn decodes_base64_urls() {
        assert_eq!(
            decode_data_url("data:text/plain;base64,SGVsbG8=").unwrap(),
            "Hello"
        );
        assert_eq!(
            decode_data_url("data:text/plain;charset=utf-8;base64,aGk=").unwrap(),
            "hi"
        );
    }

    #[test]
    fn base64_is_lenient_about_padding_and_garbage() {
        assert_eq!(decode_base64("SGVsbG8="), "Hello");
        assert_eq!(decode_base64("SGVsbG8"), "Hello");
        assert_eq!(decode_base64("SGVs bG8="), "Hello");
    }

    #[test]
    fn decodes_percent_encoded_urls() {
        assert_eq!(
            decode_data_url("data:text/plain,hello%20world").unwrap(),
            "hello world"
        );
        assert_eq!(decode_data_url("data:text/plain,a%2Bb").unwrap(), "a+b");
    }

    #[test]
    fn malformed_percent_encoding_errors() {
        assert!(decode_uri_component("%zz").is_err());
        assert!(decode_uri_component("%0").is_err());
        assert!(decode_uri_component("%c3%28").is_err());
    }

    #[test]
    fn binary_base64_decodes_as_lossy_utf8() {
        assert_eq!(
            decode_data_url("data:application/octet-stream;base64,AP8A").unwrap(),
            "\u{0}\u{FFFD}\u{0}"
        );
    }
}
