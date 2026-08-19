// From reference/packages/opencode/src/config/managed.ts.

use std::path::{Path, PathBuf};

/// Explicit managed-config directory override.
///
/// This is useful for enterprise deployments that mount policy elsewhere and
/// makes the discovery path deterministic in tests. An empty value disables
/// managed-config discovery.
pub const MANAGED_CONFIG_DIR_ENV: &str = "OPENCODE_MANAGED_CONFIG_DIR";

/// Test-only compatibility override retained for the crate's integration
/// fixtures. `OPENCODE_MANAGED_CONFIG_DIR` takes precedence when both exist.
pub const TEST_MANAGED_CONFIG_DIR_ENV: &str = "OPENCODE_TEST_MANAGED_CONFIG_DIR";

/// Keys injected by macOS/MDM into the managed plist that are not OpenCode config.
const PLIST_META: [&str; 6] = [
    "PayloadDisplayName",
    "PayloadIdentifier",
    "PayloadType",
    "PayloadUUID",
    "PayloadVersion",
    "_manualProfile",
];

/// `ConfigManaged.parseManagedPlist` — strips MDM metadata keys from the JSON
/// serialization of a managed preferences plist.
pub fn parse_managed_plist(json: &str) -> Result<String, serde_json::Error> {
    let mut value = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(json)?;
    for key in PLIST_META {
        value.shift_remove(key);
    }
    serde_json::to_string(&value)
}

/// A single node of an XML property-list, mirroring plutil's JSON conversion
/// of `<dict>/<array>/<string>/<integer>/<real>/<true|false>/<data>/<date>`.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
enum PlistNode {
    Dict(Vec<(String, PlistNode)>),
    Array(Vec<PlistNode>),
    String(String),
    Integer(i64),
    Real(f64),
    Bool(bool),
    /// Base64 payload emitted as a string, matching plutil's `<data>` form.
    Data(String),
    /// ISO8601 timestamp emitted as a string, matching plutil's `<date>` form.
    Date(String),
}

impl PlistNode {
    /// Convert into the JSON shape `plutil -convert json` emits.
    fn into_json(self) -> serde_json::Value {
        match self {
            PlistNode::Dict(entries) => serde_json::Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, value.into_json()))
                    .collect(),
            ),
            PlistNode::Array(items) => {
                serde_json::Value::Array(items.into_iter().map(PlistNode::into_json).collect())
            }
            PlistNode::String(text) => serde_json::Value::String(text),
            PlistNode::Integer(value) => serde_json::Value::from(value),
            PlistNode::Real(value) => serde_json::Value::from(value),
            PlistNode::Bool(value) => serde_json::Value::Bool(value),
            PlistNode::Data(text) => serde_json::Value::String(text),
            PlistNode::Date(text) => serde_json::Value::String(text),
        }
    }
}

/// A minimal recursive-descent reader for the XML plist subset plutil
/// supports. No external XML dependency: it walks `<dict>/<array>` containers
/// and the scalar `<string>/<integer>/<real>/<data>/<date>/<true>/<false>`
/// leaves, ignoring whitespace and the wrapping `<plist ...>` element.
#[allow(dead_code)]
struct PlistParser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> PlistParser<'a> {
    fn new(input: &'a str) -> Self {
        PlistParser { input, pos: 0 }
    }

    fn skip_ws(&mut self) {
        while let Some(ch) = self.input[self.pos..].chars().next() {
            if ch.is_ascii_whitespace() {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
    }

    /// Read an opening tag like `<dict>`, `<integer>`, or a self-closing
    /// `<true/>`, returning `(name, self_closing)`.
    fn read_open_tag(&mut self) -> Result<(String, bool), String> {
        self.skip_ws();
        if !self.input[self.pos..].starts_with('<') {
            return Err(format!("expected '<' at byte {}", self.pos));
        }
        self.pos += 1;
        let end = self.input[self.pos..]
            .find('>')
            .ok_or_else(|| "unterminated tag".to_string())?;
        let content = self.input[self.pos..self.pos + end].trim();
        self.pos += end + 1;
        let self_closing = content.ends_with('/');
        let content = content.trim_end_matches('/').trim();
        let name = content
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_string();
        if name.is_empty() {
            return Err("empty tag name".to_string());
        }
        Ok((name, self_closing))
    }

    /// Read the text up to (but not including) a closing `</name>`.
    fn read_until_close(&mut self, name: &str) -> Result<String, String> {
        let close = format!("</{name}>");
        let rest = &self.input[self.pos..];
        let end = rest
            .find(&close)
            .ok_or_else(|| format!("missing </{name}>"))?;
        let text = rest[..end].to_string();
        self.pos += end + close.len();
        Ok(text)
    }

    fn parse(&mut self) -> Result<PlistNode, String> {
        let (name, self_closing) = self.read_open_tag()?;
        match name.as_str() {
            "dict" => self.parse_dict(),
            "array" => self.parse_array(),
            "string" => Ok(PlistNode::String(self.read_until_close("string")?)),
            "integer" => {
                let text = self.read_until_close("integer")?;
                text.trim()
                    .parse::<i64>()
                    .map(PlistNode::Integer)
                    .map_err(|_| format!("invalid integer {text:?}"))
            }
            "real" => {
                let text = self.read_until_close("real")?;
                text.trim()
                    .parse::<f64>()
                    .map(PlistNode::Real)
                    .map_err(|_| format!("invalid real {text:?}"))
            }
            "true" => {
                if !self_closing {
                    return Err("<true/> must be self-closing".into());
                }
                Ok(PlistNode::Bool(true))
            }
            "false" => {
                if !self_closing {
                    return Err("<false/> must be self-closing".into());
                }
                Ok(PlistNode::Bool(false))
            }
            "data" => {
                let text = self.read_until_close("data")?;
                Ok(PlistNode::Data(
                    text.trim().replace(char::is_whitespace, ""),
                ))
            }
            "date" => Ok(PlistNode::Date(
                self.read_until_close("date")?.trim().to_string(),
            )),
            other => Err(format!("unsupported plist tag <{other}>")),
        }
    }

    fn parse_dict(&mut self) -> Result<PlistNode, String> {
        let mut entries = Vec::new();
        loop {
            self.skip_ws();
            if self.input[self.pos..].starts_with("</dict>") {
                self.pos += "</dict>".len();
                return Ok(PlistNode::Dict(entries));
            }
            let (name, _) = self.read_open_tag()?;
            if name != "key" {
                return Err(format!("expected <key> in dict, got <{name}>"));
            }
            let key = self.read_until_close("key")?;
            let value = self.parse()?;
            entries.push((key.trim().to_string(), value));
        }
    }

    fn parse_array(&mut self) -> Result<PlistNode, String> {
        let mut items = Vec::new();
        loop {
            self.skip_ws();
            if self.input[self.pos..].starts_with("</array>") {
                self.pos += "</array>".len();
                return Ok(PlistNode::Array(items));
            }
            items.push(self.parse()?);
        }
    }
}

/// Parse an XML plist into the JSON shape `plutil -convert json` emits. Used
/// on platforms without `plutil` (F055). The root element must be a `<dict>`.
#[allow(dead_code)]
fn parse_xml_plist(raw: &str) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    let xml = strip_xml_head(raw);
    let mut parser = PlistParser::new(&xml);
    let value = parser.parse()?;
    match value {
        PlistNode::Dict(entries) => Ok(entries
            .into_iter()
            .map(|(key, node)| (key, node.into_json()))
            .collect()),
        _ => Err("plist root must be a <dict>".to_string()),
    }
}

/// Strip the XML declaration, doctype, comments, and the wrapping `<plist>`
/// element so the recursive parser starts at the root `<dict>`. Real plist
/// files always carry these; hand-written fixtures may not.
#[allow(dead_code)]
fn strip_xml_head(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    loop {
        if s.starts_with("<?") {
            let Some(end) = s.find("?>") else { break };
            s = s[end + 2..].trim_start().to_string();
        } else if s.starts_with("<!--") {
            let Some(end) = s.find("-->") else { break };
            s = s[end + 3..].trim_start().to_string();
        } else if s.starts_with("<!DOCTYPE") || s.starts_with("<!doctype") {
            let Some(end) = s.find('>') else { break };
            s = s[end + 1..].trim_start().to_string();
        } else if s.starts_with("<plist") {
            let Some(close) = s.rfind("</plist>") else {
                break;
            };
            let open_end = s.find('>').map(|index| index + 1).unwrap_or(0);
            s = s[open_end..close].trim().to_string();
        } else {
            break;
        }
    }
    s
}

/// Whether `raw` looks like JSON rather than XML plist.
#[allow(dead_code)]
fn looks_like_json(raw: &str) -> bool {
    let trimmed = raw.trim_start();
    trimmed.starts_with('{') || trimmed.starts_with('[')
}

/// Returns the system-managed configuration directory for the current OS.
///
/// The environment overrides are checked before platform defaults so callers
/// can exercise every platform's loading behavior without changing the host
/// operating system.
pub fn managed_config_dir() -> Option<PathBuf> {
    if let Some(value) = env_path(MANAGED_CONFIG_DIR_ENV) {
        return value;
    }
    if let Some(value) = env_path(TEST_MANAGED_CONFIG_DIR_ENV) {
        return value;
    }

    #[cfg(target_os = "macos")]
    {
        Some(PathBuf::from("/Library/Managed Preferences"))
    }
    #[cfg(target_os = "windows")]
    {
        Some(
            PathBuf::from(
                std::env::var_os("ProgramData").unwrap_or_else(|| "C:\\ProgramData".into()),
            )
            .join("opencode"),
        )
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Some(PathBuf::from("/etc/opencode"))
    }
}

fn env_path(name: &str) -> Option<Option<PathBuf>> {
    match std::env::var_os(name) {
        None => None,
        Some(value) if value.is_empty() => Some(None),
        Some(value) => Some(Some(PathBuf::from(value))),
    }
}

/// Candidate files are ordered from lowest to highest precedence.
///
/// JSON files are supported on all platforms. The plist names cover the
/// usual macOS MDM payload names and are also accepted under an override on
/// other platforms when their contents are JSON, which keeps tests portable.
pub fn config_files(directory: &Path) -> Vec<PathBuf> {
    [
        "config.json",
        "opencode.json",
        "opencode.jsonc",
        "opencode.plist",
        "ai.opencode.plist",
        "com.opencode.plist",
    ]
    .into_iter()
    .map(|name| directory.join(name))
    .collect()
}

/// Reads a managed config file and normalizes plist metadata away.
///
/// macOS uses `plutil` because it is the system's canonical plist parser. On
/// other platforms a plist candidate may still contain JSON (handy for
/// portable deployment tests), while XML plist input returns a clear error.
pub fn read_config(path: &Path) -> std::io::Result<String> {
    let is_plist = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("plist"));
    if !is_plist {
        return std::fs::read_to_string(path);
    }

    let raw = std::fs::read_to_string(path)?;

    #[cfg(target_os = "macos")]
    let json = {
        let output = std::process::Command::new("plutil")
            .args(["-convert", "json", "-o", "-", "--"])
            .arg(path)
            .output()?;
        if output.status.success() {
            String::from_utf8(output.stdout).map_err(|error| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
            })?
        } else {
            // Keep JSON-encoded fixtures and enterprise export files usable
            // on macOS even when they are given a `.plist` suffix.
            raw.clone()
        }
    };

    #[cfg(not(target_os = "macos"))]
    let json = if looks_like_json(&raw) {
        raw
    } else {
        // F055: on platforms without `plutil`, decode XML plist with the
        // minimal built-in parser into the same JSON shape plutil emits.
        match parse_xml_plist(&raw)
            .and_then(|map| serde_json::to_string(&map).map_err(|e| e.to_string()))
        {
            Ok(json) => json,
            Err(error) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("managed plist is not valid JSON or XML plist: {error}"),
                ));
            }
        }
    };

    parse_managed_plist(&json).map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("managed plist is not valid JSON: {error}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{config_files, parse_managed_plist, parse_xml_plist, read_config};
    use std::path::Path;

    #[test]
    fn strips_mdm_metadata_keys() {
        let out = parse_managed_plist(
            r#"{"PayloadDisplayName":"x","PayloadUUID":"u","_manualProfile":true,"share":"disabled","model":"mdm/model"}"#,
        )
        .expect("parse");
        let value: serde_json::Value = serde_json::from_str(&out).expect("json");
        assert_eq!(value["share"], "disabled");
        assert_eq!(value["model"], "mdm/model");
        assert!(value.get("PayloadUUID").is_none());
        assert!(value.get("_manualProfile").is_none());
    }

    #[test]
    fn discovers_cross_platform_candidate_names_in_precedence_order() {
        let files = config_files(Path::new("/managed"));
        let names: Vec<_> = files
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(
            names,
            [
                "config.json",
                "opencode.json",
                "opencode.jsonc",
                "opencode.plist",
                "ai.opencode.plist",
                "com.opencode.plist"
            ]
        );
    }

    #[test]
    fn reads_json_encoded_plist_on_every_platform() {
        let temp = tempfile::tempdir().expect("temp dir");
        let path = temp.path().join("ai.opencode.plist");
        std::fs::write(
            &path,
            r#"{"PayloadType":"Configuration","PayloadUUID":"uuid","model":"mdm/model"}"#,
        )
        .expect("write plist fixture");
        let value: serde_json::Value =
            serde_json::from_str(&read_config(&path).expect("read plist")).expect("json");
        assert_eq!(value["model"], "mdm/model");
        assert!(value.get("PayloadUUID").is_none());
    }

    #[test]
    fn parses_nested_xml_plist_dict_and_array() {
        let xml = r#"<plist version="1.0">
          <dict>
            <key>name</key><string>opencode</string>
            <key>count</key><integer>42</integer>
            <key>ratio</key><real>1.5</real>
            <key>enabled</key><true/>
            <key>off</key><false/>
            <key>stamp</key><date>2024-01-01T00:00:00Z</date>
            <key>nested</key>
            <dict><key>inner</key><array><string>a</string><integer>1</integer></array></dict>
            <key>blob</key><data>AAAA</data>
          </dict>
        </plist>"#;
        let map = parse_xml_plist(xml).expect("parse xml plist");
        assert_eq!(map["name"], serde_json::Value::String("opencode".into()));
        assert_eq!(map["count"], serde_json::Value::from(42));
        assert_eq!(map["ratio"], serde_json::Value::from(1.5));
        assert_eq!(map["enabled"], serde_json::Value::Bool(true));
        assert_eq!(map["off"], serde_json::Value::Bool(false));
        assert_eq!(
            map["stamp"],
            serde_json::Value::String("2024-01-01T00:00:00Z".into())
        );
        assert_eq!(map["blob"], serde_json::Value::String("AAAA".into()));
        let nested = map["nested"].as_object().expect("nested dict");
        let inner = nested["inner"].as_array().expect("inner array");
        assert_eq!(inner[0], "a");
        assert_eq!(inner[1], 1);
    }

    #[test]
    fn xml_plist_strips_mdm_metadata_keys() {
        let xml = r#"<dict>
            <key>PayloadUUID</key><string>uuid</string>
            <key>PayloadType</key><string>Configuration</string>
            <key>_manualProfile</key><true/>
            <key>model</key><string>mdm/model</string>
        </dict>"#;
        let map = parse_xml_plist(xml).expect("parse xml plist");
        let json = serde_json::to_string(&serde_json::Value::Object(map)).unwrap();
        let out = parse_managed_plist(&json).expect("strip metadata");
        let value: serde_json::Value = serde_json::from_str(&out).expect("json");
        assert_eq!(value["model"], "mdm/model");
        assert!(value.get("PayloadUUID").is_none());
        assert!(value.get("_manualProfile").is_none());
    }

    #[test]
    fn xml_plist_parser_errors_on_unknown_or_malformed_input() {
        assert!(
            parse_xml_plist("<dict><key>k</key><bogus>1</bogus></dict>").is_err(),
            "unknown tag must error"
        );
        assert!(
            parse_xml_plist("<dict><key>k</key><integer>not-a-number</integer></dict>").is_err(),
            "malformed integer must error"
        );
        assert!(
            parse_xml_plist("<array><string>root-not-dict</string></array>").is_err(),
            "non-dict root must error"
        );
    }
}
