/// From reference/packages/opencode/src/util/bom.ts
///
/// Byte-order-mark handling. `read_file` mirrors
/// `new TextDecoder("utf-8", { ignoreBOM: true })` — the BOM is decoded as a
/// `\u{FEFF}` character rather than stripped, then `split` detects it.
pub const BOM_CHAR: char = '\u{FEFF}';

pub fn split(text: &str) -> (bool, &str) {
    if let Some(stripped) = text.strip_prefix(BOM_CHAR) {
        (true, stripped)
    } else {
        (false, text)
    }
}

pub fn join(text: &str, bom: bool) -> String {
    let (_, stripped) = split(text);
    if bom {
        let mut out = String::with_capacity(stripped.len() + 3);
        out.push(BOM_CHAR);
        out.push_str(stripped);
        out
    } else {
        stripped.to_string()
    }
}

pub fn read_file(bytes: &[u8]) -> (bool, String) {
    let decoded = String::from_utf8_lossy(bytes);
    let (bom, text) = split(&decoded);
    (bom, text.to_string())
}

/// Syncs the BOM of `file_path` on disk to the requested `bom` state, mirroring
/// `Bom.syncFile`. Returns the current file text.
pub async fn sync_file(file_path: &str, bom: bool) -> anyhow::Result<String> {
    let bytes = crate::fs_util::read_file_bytes(file_path).await?;
    let (has_bom, text) = read_file(&bytes);
    if has_bom != bom {
        crate::fs_util::write_with_dirs(file_path, join(&text, bom).into_bytes(), None).await?;
    }
    Ok(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_detects_bom() {
        assert_eq!(split("\u{FEFF}hello"), (true, "hello"));
        assert_eq!(split("hello"), (false, "hello"));
        assert_eq!(split(""), (false, ""));
    }

    #[test]
    fn join_reinserts_bom() {
        assert_eq!(join("hello", true), "\u{FEFF}hello");
        assert_eq!(join("hello", false), "hello");
        assert_eq!(join("\u{FEFF}hello", true), "\u{FEFF}hello");
        assert_eq!(join("\u{FEFF}hello", false), "hello");
    }

    #[test]
    fn read_file_keeps_bom_character_for_detection() {
        assert_eq!(read_file(b"\xef\xbb\xbfhi"), (true, "hi".to_string()));
        assert_eq!(read_file(b"hi"), (false, "hi".to_string()));
    }
}
