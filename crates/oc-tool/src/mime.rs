//! `FSUtil.mimeType` (`reference/packages/core/src/fs-util.ts:224`) —
//! extension-based MIME lookup with an `application/octet-stream` fallback.

pub fn mime_type(path: &str) -> String {
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .unwrap_or_default();
    mime_from_extension(&extension)
        .map(|mime| mime.to_string())
        .unwrap_or_else(|| "application/octet-stream".to_string())
}

fn mime_from_extension(extension: &str) -> Option<&'static str> {
    Some(match extension {
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "csv" => "text/csv",
        "txt" => "text/plain",
        "md" | "markdown" => "text/markdown",
        "json" => "application/json",
        "js" => "application/javascript",
        "mjs" => "application/javascript",
        "ts" => "application/typescript",
        "tsx" => "application/typescript",
        "jsx" => "application/javascript",
        "xml" => "application/xml",
        "yaml" | "yml" => "application/yaml",
        "toml" => "application/toml",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "tar" => "application/x-tar",
        "7z" => "application/x-7z-compressed",
        "rar" => "application/vnd.rar",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "eot" => "application/vnd.ms-fontobject",
        "wasm" => "application/wasm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "sh" => "application/x-sh",
        "py" => "text/x-python",
        "rs" => "text/x-rust",
        "go" => "text/x-go",
        "c" => "text/x-c",
        "h" => "text/x-c",
        "cpp" => "text/x-c++",
        "hpp" => "text/x-c++",
        "java" => "text/x-java",
        "rb" => "text/x-ruby",
        "php" => "text/x-php",
        "swift" => "text/x-swift",
        "kt" => "text/x-kotlin",
        "sql" => "text/x-sql",
        "log" => "text/x-log",
        "ini" => "text/plain",
        "conf" => "text/plain",
        "env" => "text/plain",
        "lock" => "text/plain",
        "diff" | "patch" => "text/x-diff",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_common_extensions() {
        assert_eq!(mime_type("/a/b/file.ts"), "application/typescript");
        assert_eq!(mime_type("/a/b/image.png"), "image/png");
        assert_eq!(mime_type("/a/b/noext"), "application/octet-stream");
    }
}
