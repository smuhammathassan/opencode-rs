/// Small shared helpers: base64 (url-safe), HTTP-date parsing, JSON data-url
/// decoding. Implemented locally to keep oc-session dependency-lean.

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const B64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn encode_raw(input: &[u8], table: &[u8; 64], padding: bool) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    let mut chunks = input.chunks_exact(3);
    for chunk in &mut chunks {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(chunk[1]) << 8) | u32::from(chunk[2]);
        out.push(table[(n >> 18) as usize & 63] as char);
        out.push(table[(n >> 12) as usize & 63] as char);
        out.push(table[(n >> 6) as usize & 63] as char);
        out.push(table[n as usize & 63] as char);
    }
    let rem = chunks.remainder();
    if rem.is_empty() {
        return out;
    }
    let mut n = u32::from(rem[0]) << 16;
    if rem.len() > 1 {
        n |= u32::from(rem[1]) << 8;
    }
    out.push(table[(n >> 18) as usize & 63] as char);
    out.push(table[(n >> 12) as usize & 63] as char);
    if rem.len() == 2 {
        out.push(table[(n >> 6) as usize & 63] as char);
    }
    if padding {
        for _ in 0..(4 - (rem.len() + 1)) {
            out.push('=');
        }
    }
    out
}

/// Base64 with padding (standard alphabet) — used for `data:` URLs.
pub fn base64_encode(input: &[u8]) -> String {
    encode_raw(input, B64, true)
}

/// Base64url without padding — used by `MessageV2.cursor`.
pub fn base64url_encode(input: &[u8]) -> String {
    encode_raw(input, B64URL, false)
}

fn decode_raw(input: &str, table: &[u8; 64]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits = 0;
    for c in input.bytes().filter(|c| !c.is_ascii_whitespace()) {
        if c == b'=' {
            break;
        }
        let value = table.iter().position(|x| *x == c)? as u32;
        acc = (acc << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// Base64 decode (standard alphabet, tolerant of whitespace/padding).
pub fn base64_decode(input: &str) -> Option<Vec<u8>> {
    decode_raw(input, B64)
}

/// Base64url decode without padding.
pub fn base64url_decode(input: &str) -> Option<Vec<u8>> {
    let mut s = input.to_string();
    s.retain(|c| c != '=');
    decode_raw(&s, B64URL)
}

/// From reference `packages/opencode/src/util/data-url.ts` — decodes
/// `data:[mime];base64,<payload>` into the raw text.
pub fn decode_data_url(url: &str) -> Option<String> {
    let comma = url.find(',')?;
    let meta = &url[..comma];
    let payload = &url[comma + 1..];
    if !meta.ends_with(";base64") {
        return None;
    }
    let bytes = base64_decode(payload)?;
    String::from_utf8(bytes).ok()
}

/// Parse an RFC 7231 HTTP-date into unix epoch milliseconds (0 when invalid).
pub fn http_date_to_unix_millis(value: &str) -> u64 {
    let value = value.trim();
    // RFC 1123: "Sun, 06 Nov 1994 08:49:37 GMT"
    let parsed = chrono_like_rfc1123(value)
        .or_else(|| chrono_like_rfc850(value))
        .or_else(|| chrono_like_asctime(value));
    parsed.map(|secs| secs * 1000).unwrap_or(0)
}

fn chrono_like_rfc1123(value: &str) -> Option<u64> {
    // Parse "%a, %d %b %Y %H:%M:%S GMT" manually.
    let parts: Vec<&str> = value.split_whitespace().collect();
    if parts.len() < 6 {
        return None;
    }
    let day = parts[1].parse::<u32>().ok()?;
    let month = month_number(parts[2])?;
    let year = parts[3].parse::<i64>().ok()?;
    let time: Vec<&str> = parts[4].split(':').collect();
    if time.len() != 3 {
        return None;
    }
    let (h, m, s) = (
        time[0].parse::<u64>().ok()?,
        time[1].parse::<u64>().ok()?,
        time[2].parse::<u64>().ok()?,
    );
    Some(days_from_civil(year, month, day) * 86_400 + h * 3600 + m * 60 + s)
}

fn chrono_like_rfc850(value: &str) -> Option<u64> {
    let parts: Vec<&str> = value.split_whitespace().collect();
    if parts.len() < 6 {
        return None;
    }
    // "Sunday, 06-Nov-94 08:49:37 GMT"
    let day_str = parts[1].trim_end_matches(',');
    let sub: Vec<&str> = day_str.split('-').collect();
    if sub.len() != 3 {
        return None;
    }
    let day = sub[0].parse::<u32>().ok()?;
    let month = month_number(sub[1])?;
    let mut year = sub[2].parse::<i64>().ok()?;
    if year < 70 {
        year += 2000;
    } else if year < 100 {
        year += 1900;
    }
    let time: Vec<&str> = parts[4].split(':').collect();
    let (h, m, s) = (
        time[0].parse::<u64>().ok()?,
        time[1].parse::<u64>().ok()?,
        time[2].parse::<u64>().ok()?,
    );
    Some(days_from_civil(year, month, day) * 86_400 + h * 3600 + m * 60 + s)
}

fn chrono_like_asctime(value: &str) -> Option<u64> {
    // "Sun Nov  6 08:49:37 1994"
    let parts: Vec<&str> = value.split_whitespace().collect();
    if parts.len() < 5 {
        return None;
    }
    let month = month_number(parts[1])?;
    let day = parts[2].parse::<u32>().ok()?;
    let year = parts[4].parse::<i64>().ok()?;
    let time: Vec<&str> = parts[3].split(':').collect();
    let (h, m, s) = (
        time[0].parse::<u64>().ok()?,
        time[1].parse::<u64>().ok()?,
        time[2].parse::<u64>().ok()?,
    );
    Some(days_from_civil(year, month, day) * 86_400 + h * 3600 + m * 60 + s)
}

fn month_number(name: &str) -> Option<i64> {
    Some(match name.to_ascii_lowercase().as_str() {
        "jan" => 1,
        "feb" => 2,
        "mar" => 3,
        "apr" => 4,
        "may" => 5,
        "jun" => 6,
        "jul" => 7,
        "aug" => 8,
        "sep" => 9,
        "oct" => 10,
        "nov" => 11,
        "dec" => 12,
        _ => return None,
    })
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's
/// algorithm).
fn days_from_civil(y: i64, m: i64, d: u32) -> u64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe - 719_468) as u64
}

/// From reference `packages/core/src/util/slug.ts` — `adjective-noun` slugs.
pub mod slug {
    const ADJECTIVES: [&str; 29] = [
        "brave", "calm", "clever", "cosmic", "crisp", "curious", "eager", "gentle", "glowing",
        "happy", "hidden", "jolly", "kind", "lucky", "mighty", "misty", "neon", "nimble",
        "playful", "proud", "quick", "quiet", "shiny", "silent", "stellar", "sunny", "swift",
        "tidy", "witty",
    ];
    const NOUNS: [&str; 31] = [
        "cabin", "cactus", "canyon", "circuit", "comet", "eagle", "engine", "falcon", "forest",
        "garden", "harbor", "island", "knight", "lagoon", "meadow", "moon", "mountain", "nebula",
        "orchid", "otter", "panda", "pixel", "planet", "river", "rocket", "sailor", "squid",
        "star", "tiger", "wizard", "wolf",
    ];

    pub fn create() -> String {
        let idx = rand_index(ADJECTIVES.len());
        let noun_idx = rand_index(NOUNS.len());
        format!("{}-{}", ADJECTIVES[idx], NOUNS[noun_idx])
    }

    fn rand_index(len: usize) -> usize {
        let mut seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0)
            ^ (std::process::id() as u64).wrapping_mul(0x9E3779B97F4A7C15);
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed as usize) % len
    }
}

/// From reference `packages/opencode/src/util/token.ts` — crude token estimate
/// used by compaction. The reference counts chars/4 per string.
pub mod token {
    /// From reference `util/token.ts:estimate`. Estimates tokens from JSON
    /// text: 4 characters per token, with a 0.9x factor for JSON formatting.
    pub fn estimate(value: &str) -> u64 {
        let chars = value.chars().count() as f64;
        (chars / 4.0 * 0.9) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trip() {
        assert_eq!(base64_encode(b"hello world"), "aGVsbG8gd29ybGQ=");
        assert_eq!(base64_decode("aGVsbG8gd29ybGQ=").unwrap(), b"hello world");
    }

    #[test]
    fn base64url_round_trip() {
        let encoded = base64url_encode(&[0xfb, 0xff, 0xff]);
        assert_eq!(encoded, "-___");
        assert_eq!(base64url_decode(&encoded).unwrap(), vec![0xfb, 0xff, 0xff]);
    }

    #[test]
    fn http_date_rfc1123() {
        let ms = http_date_to_unix_millis("Sun, 06 Nov 1994 08:49:37 GMT");
        assert_eq!(ms, 784_111_777_000);
    }

    #[test]
    fn decode_data_url_plain() {
        let url = "data:text/plain;base64,aGVsbG8=";
        assert_eq!(decode_data_url(url).as_deref(), Some("hello"));
    }

    #[test]
    fn token_estimate() {
        let value = "a".repeat(40);
        assert_eq!(token::estimate(&value), 9);
    }
}
