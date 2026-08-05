fn main() {
    let dir = std::env::temp_dir().join(format!("oc-meta-dbg-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let r = oc_plugin::meta::touch(&dir, "my-plugin", "/cache/packages/my-plugin", "my-plugin");
    eprintln!(
        "touch: {:?}",
        r.as_ref()
            .map(|(s, e)| (s, e.fingerprint.clone(), e.load_count))
    );
    let file = oc_plugin::meta::store_path(&dir);
    eprintln!("file exists: {}", file.exists());
    let text = std::fs::read_to_string(&file).unwrap_or_else(|e| format!("ERR {e}"));
    eprintln!(
        "content len {}: {:?}",
        text.len(),
        &text[..text.len().min(120)]
    );
}
