//! Amazon Bedrock provider facade.
//! From reference/packages/llm/src/providers/amazon-bedrock.ts

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::protocols::bedrock_converse;
use crate::route::auth::{Auth, AuthInput, Credential, HeaderMap};
use crate::route::{EndpointPatch, Route, RouteModelInput, RoutePatch};
use crate::schema::{GenerationOptions, HttpOptions, Model, ModelLimits, ProviderOptions};
use url::Url;

pub const ID: &str = "amazon-bedrock";

/// `BedrockCredentials`.
/// From reference/packages/llm/src/protocols/utils/bedrock-auth.ts (`Credentials`)
#[derive(Debug, Clone, Default)]
pub struct BedrockCredentials {
    pub region: Option<String>,
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
    pub session_token: Option<String>,
}

/// `Config`.
/// From reference/packages/llm/src/providers/amazon-bedrock.ts (`Config`)
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub api_key: Option<String>,
    pub headers: Option<BTreeMap<String, String>>,
    pub credentials: Option<BedrockCredentials>,
    pub region: Option<String>,
    pub base_url: Option<String>,
    pub limits: Option<ModelLimits>,
    pub generation: Option<GenerationOptions>,
    pub provider_options: Option<ProviderOptions>,
    pub http: Option<HttpOptions>,
}

fn bedrock_base_url(region: &str) -> String {
    format!("https://bedrock-runtime.{}.amazonaws.com", region)
}

/// SigV4 route auth.
/// From reference/packages/llm/src/protocols/utils/bedrock-auth.ts (`sigV4`)
pub fn sig_v4_auth(credentials: Option<&BedrockCredentials>) -> Auth {
    let credentials = credentials.cloned();
    crate::route::auth::custom(move |input| sign_request(input, credentials.as_ref()))
}

fn sign_request(
    input: &AuthInput,
    configured: Option<&BedrockCredentials>,
) -> Result<HeaderMap, crate::schema::LlmError> {
    let url = Url::parse(&input.url).map_err(|error| {
        crate::shared::invalid_request(format!("Bedrock request URL is invalid: {error}"))
    })?;
    let region = configured
        .and_then(|value| value.region.clone())
        .or_else(|| std::env::var("AWS_REGION").ok())
        .or_else(|| std::env::var("AWS_DEFAULT_REGION").ok())
        .unwrap_or_else(|| "us-east-1".to_string());
    let access_key = configured
        .and_then(|value| value.access_key_id.clone())
        .or_else(|| std::env::var("AWS_ACCESS_KEY_ID").ok())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            crate::shared::invalid_request(
                "Bedrock Converse requires AWS_ACCESS_KEY_ID or configured access_key_id",
            )
        })?;
    let secret_key = configured
        .and_then(|value| value.secret_access_key.clone())
        .or_else(|| std::env::var("AWS_SECRET_ACCESS_KEY").ok())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            crate::shared::invalid_request(
                "Bedrock Converse requires AWS_SECRET_ACCESS_KEY or configured secret_access_key",
            )
        })?;
    let session_token = configured
        .and_then(|value| value.session_token.clone())
        .or_else(|| std::env::var("AWS_SESSION_TOKEN").ok())
        .filter(|value| !value.is_empty());

    let mut headers = BTreeMap::new();
    for (key, value) in &input.headers {
        let key = key.to_ascii_lowercase();
        if key != "authorization" {
            headers.insert(key, value.clone());
        }
    }
    let host = url
        .host_str()
        .ok_or_else(|| crate::shared::invalid_request("Bedrock request URL has no host"))?;
    headers.insert("host".into(), host.to_string());

    let (amz_date, date) = match headers.get("x-amz-date") {
        Some(value) if value.len() >= 16 => (value.clone(), value[..8].to_string()),
        _ => current_amz_time(),
    };
    headers.insert("x-amz-date".into(), amz_date.clone());
    let payload_hash = sha256_hex(input.body.as_bytes());
    headers.insert("x-amz-content-sha256".into(), payload_hash.clone());
    if let Some(token) = &session_token {
        headers.insert("x-amz-security-token".into(), token.clone());
    }

    let canonical_headers = headers
        .iter()
        .map(|(key, value)| format!("{key}:{}\n", canonical_header(value)))
        .collect::<String>();
    let signed_headers = headers.keys().cloned().collect::<Vec<_>>().join(";");
    let canonical_uri = canonical_uri(&url);
    let canonical_query = canonical_query(&url);
    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        input.method.to_ascii_uppercase(),
        canonical_uri,
        canonical_query,
        canonical_headers,
        signed_headers,
        payload_hash,
    );
    let scope = format!("{date}/{region}/bedrock/aws4_request");
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        sha256_hex(canonical_request.as_bytes())
    );
    let date_key = hmac_sha256(format!("AWS4{secret_key}").as_bytes(), date.as_bytes());
    let region_key = hmac_sha256(&date_key, region.as_bytes());
    let service_key = hmac_sha256(&region_key, b"bedrock");
    let signing_key = hmac_sha256(&service_key, b"aws4_request");
    let signature = hex_encode(&hmac_sha256(&signing_key, string_to_sign.as_bytes()));

    headers.insert(
        "authorization".into(),
        format!(
            "AWS4-HMAC-SHA256 Credential={access_key}/{scope}, SignedHeaders={signed_headers}, Signature={signature}"
        ),
    );
    Ok(headers)
}

fn canonical_header(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn canonical_uri(url: &Url) -> String {
    let path = url.path();
    if path.is_empty() {
        return "/".into();
    }
    let bytes = path.as_bytes();
    let mut output = String::with_capacity(path.len());
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'/' || byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~')
        {
            output.push(byte as char);
            index += 1;
        } else if byte == b'%'
            && index + 2 < bytes.len()
            && is_hex(bytes[index + 1])
            && is_hex(bytes[index + 2])
        {
            output.push('%');
            output.push((bytes[index + 1] as char).to_ascii_uppercase());
            output.push((bytes[index + 2] as char).to_ascii_uppercase());
            index += 3;
        } else {
            output.push('%');
            output.push_str(&format!("{byte:02X}"));
            index += 1;
        }
    }
    output
}

fn canonical_query(url: &Url) -> String {
    let mut pairs = url
        .query_pairs()
        .map(|(key, value)| (percent_encode(&key, true), percent_encode(&value, true)))
        .collect::<Vec<_>>();
    pairs.sort();
    pairs
        .into_iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&")
}

fn percent_encode(value: &str, encode_slash: bool) -> String {
    let mut output = String::new();
    for byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(*byte, b'-' | b'_' | b'.' | b'~')
            || (!encode_slash && *byte == b'/')
        {
            output.push(*byte as char);
        } else {
            output.push('%');
            output.push_str(&format!("{byte:02X}"));
        }
    }
    output
}

fn is_hex(value: u8) -> bool {
    value.is_ascii_hexdigit()
}

fn current_amz_time() -> (String, String) {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or_default();
    let days = (seconds / 86_400) as i64;
    let remaining = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = remaining / 3_600;
    let minute = remaining % 3_600 / 60;
    let second = remaining % 60;
    let date = format!("{year:04}{month:02}{day:02}");
    (format!("{date}T{hour:02}{minute:02}{second:02}Z"), date)
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let month_part = (5 * doy + 2) / 153;
    let day = doy - (153 * month_part + 2) / 5 + 1;
    let month = if month_part < 10 {
        month_part + 3
    } else {
        month_part - 9
    };
    (
        if month <= 2 { year + 1 } else { year },
        month as u32,
        day as u32,
    )
}

fn sha256_hex(value: &[u8]) -> String {
    oc_core::util::hash::sha256(value)
}

fn sha256_raw(value: &[u8]) -> [u8; 32] {
    let encoded = sha256_hex(value);
    let mut result = [0_u8; 32];
    for (index, pair) in encoded.as_bytes().chunks_exact(2).enumerate() {
        result[index] = (hex_value(pair[0]) << 4) | hex_value(pair[1]);
    }
    result
}

fn hex_value(value: u8) -> u8 {
    match value {
        b'0'..=b'9' => value - b'0',
        b'a'..=b'f' => value - b'a' + 10,
        b'A'..=b'F' => value - b'A' + 10,
        _ => 0,
    }
}

fn hex_encode(value: &[u8]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hmac_sha256(key: &[u8], value: &[u8]) -> [u8; 32] {
    let mut normalized = [0_u8; 64];
    if key.len() > normalized.len() {
        normalized[..32].copy_from_slice(&sha256_raw(key));
    } else {
        normalized[..key.len()].copy_from_slice(key);
    }
    let mut inner = Vec::with_capacity(64 + value.len());
    let mut outer = Vec::with_capacity(64 + 32);
    for byte in normalized {
        inner.push(byte ^ 0x36);
        outer.push(byte ^ 0x5c);
    }
    inner.extend_from_slice(value);
    outer.extend_from_slice(&sha256_raw(&inner));
    sha256_raw(&outer)
}

/// `AmazonBedrock.configure(input)`.
/// From reference/packages/llm/src/providers/amazon-bedrock.ts (`configure`)
pub fn configure(input: Config) -> BedrockProvider {
    let resolved_region = input
        .region
        .clone()
        .or_else(|| input.credentials.as_ref().and_then(|c| c.region.clone()))
        .unwrap_or_else(|| "us-east-1".to_string());
    let mut patch = RoutePatch::empty();
    patch.provider = Some(ID.to_string());
    patch.endpoint = Some(EndpointPatch::base_url(
        input
            .base_url
            .clone()
            .unwrap_or_else(|| bedrock_base_url(&resolved_region)),
    ));
    patch.auth = Some(match &input.api_key {
        Some(api_key) => Credential::Value(api_key.clone()).bearer_auth(),
        None => sig_v4_auth(input.credentials.as_ref()),
    });
    patch.headers = input.headers.clone();
    patch.limits = input.limits.clone();
    patch.generation = input.generation.clone();
    patch.provider_options = input.provider_options.clone();
    patch.http = input.http.clone();
    let route = Arc::new(bedrock_converse::route().with(patch));
    let model = move |model_id: String| -> Model {
        route
            .model(RouteModelInput {
                id: model_id,
                provider: Some(ID.to_string()),
                defaults: None,
                compatibility: None,
            })
            .unwrap()
    };
    BedrockProvider {
        id: ID.to_string(),
        model: Arc::new(model),
    }
}

/// Default provider (env-based credentials).
/// From reference/packages/llm/src/providers/amazon-bedrock.ts (`provider`)
pub fn provider() -> BedrockProvider {
    configure(Config::default())
}

/// Provider handle.
#[derive(Clone)]
pub struct BedrockProvider {
    pub id: String,
    pub model: Arc<dyn Fn(String) -> Model + Send + Sync>,
}

impl BedrockProvider {
    pub fn model(&self, id: impl Into<String>) -> Model {
        (self.model)(id.into())
    }
}

/// `routes`.
/// From reference/packages/llm/src/providers/amazon-bedrock.ts (`routes`)
pub fn routes() -> Vec<Route> {
    vec![bedrock_converse::route()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_sha256_matches_rfc4231_vector() {
        let key = [0x0b_u8; 20];
        let digest = hmac_sha256(&key, b"Hi There");
        assert_eq!(
            hex_encode(&digest),
            "b0344c61d8db38535ca8afceaf0bf12b".to_string() + "881dc200c9833da726e9376c2e32cff7"
        );
    }

    #[test]
    fn canonical_path_and_query_are_aws_encoded_and_sorted() {
        let url = Url::parse(
            "https://bedrock-runtime.us-east-1.amazonaws.com/model/foo%2Fbar/converse?z=hello world&a=1/2",
        )
        .unwrap();
        assert_eq!(canonical_uri(&url), "/model/foo%2Fbar/converse");
        assert_eq!(canonical_query(&url), "a=1%2F2&z=hello%20world");
    }

    #[test]
    fn current_amz_time_has_sigv4_shapes() {
        let (timestamp, date) = current_amz_time();
        assert_eq!(timestamp.len(), 16);
        assert_eq!(date.len(), 8);
        assert!(timestamp.ends_with('Z'));
    }
}
