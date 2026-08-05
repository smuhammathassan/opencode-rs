/// From reference/packages/opencode/src/util/media.ts
fn starts_with(bytes: &[u8], prefix: &[u8]) -> bool {
    bytes.len() >= prefix.len() && prefix.iter().enumerate().all(|(i, p)| bytes[i] == *p)
}

pub fn is_pdf_attachment(mime: &str) -> bool {
    mime == "application/pdf"
}

pub fn is_media(mime: &str) -> bool {
    mime.starts_with("image/") || is_pdf_attachment(mime)
}

pub fn is_image_attachment(mime: &str) -> bool {
    mime.starts_with("image/") && mime != "image/svg+xml" && mime != "image/vnd.fastbidsheet"
}

/// From reference/packages/opencode/src/util/media.ts
///
/// The reference sniffs the leading magic bytes of an attachment and falls
/// back to `fallback` when nothing matches. The webp check mirrors the JS
/// `bytes.subarray(8)` behavior: an empty subarray "matches" any prefix.
pub fn sniff_attachment_mime<'a>(bytes: &[u8], fallback: &'a str) -> &'a str {
    if starts_with(bytes, &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]) {
        return "image/png";
    }
    if starts_with(bytes, &[0xff, 0xd8, 0xff]) {
        return "image/jpeg";
    }
    if starts_with(bytes, &[0x47, 0x49, 0x46, 0x38]) {
        return "image/gif";
    }
    if starts_with(bytes, &[0x42, 0x4d]) {
        return "image/bmp";
    }
    if starts_with(bytes, &[0x25, 0x50, 0x44, 0x46, 0x2d]) {
        return "application/pdf";
    }
    if starts_with(bytes, &[0x52, 0x49, 0x46, 0x46]) {
        let subarray = &bytes[bytes.len().min(8)..];
        if starts_with(subarray, &[0x57, 0x45, 0x42, 0x50]) {
            return "image/webp";
        }
    }
    fallback
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_png() {
        let bytes = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00];
        assert_eq!(sniff_attachment_mime(&bytes, "fallback"), "image/png");
    }

    #[test]
    fn sniffs_jpeg() {
        assert_eq!(
            sniff_attachment_mime(&[0xff, 0xd8, 0xff, 0xe0], "f"),
            "image/jpeg"
        );
    }

    #[test]
    fn sniffs_gif_bmp_pdf() {
        assert_eq!(sniff_attachment_mime(b"GIF89a", "f"), "image/gif");
        assert_eq!(sniff_attachment_mime(b"BM", "f"), "image/bmp");
        assert_eq!(sniff_attachment_mime(b"%PDF-1.4", "f"), "application/pdf");
    }

    #[test]
    fn sniffs_webp() {
        let mut bytes = vec![0x52, 0x49, 0x46, 0x46, 0, 0, 0, 0, 0x57, 0x45, 0x42, 0x50];
        assert_eq!(sniff_attachment_mime(&bytes, "f"), "image/webp");
        bytes.truncate(4);
        assert_eq!(sniff_attachment_mime(&bytes, "f"), "f");
        bytes.truncate(3);
        assert_eq!(sniff_attachment_mime(&bytes, "f"), "f");
    }

    #[test]
    fn unknown_bytes_fall_back() {
        assert_eq!(
            sniff_attachment_mime(&[0x00, 0x01], "application/octet-stream"),
            "application/octet-stream"
        );
    }

    #[test]
    fn media_checks() {
        assert!(is_media("image/png"));
        assert!(is_media("application/pdf"));
        assert!(!is_media("text/plain"));
        assert!(is_image_attachment("image/png"));
        assert!(!is_image_attachment("image/svg+xml"));
        assert!(!is_image_attachment("image/vnd.fastbidsheet"));
        assert!(is_image_attachment("image/jpeg"));
    }
}
