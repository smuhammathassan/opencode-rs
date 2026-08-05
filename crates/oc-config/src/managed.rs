// From reference/packages/opencode/src/config/managed.ts (pure parts)

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

#[cfg(test)]
mod tests {
    use super::parse_managed_plist;

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
}
