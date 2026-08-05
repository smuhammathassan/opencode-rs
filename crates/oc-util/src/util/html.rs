/// From reference/packages/opencode/src/util/html.ts
pub fn escape_html(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::escape_html;

    #[test]
    fn escapes_all_special_characters() {
        assert_eq!(
            escape_html("<a href=\"x\" title='y'>a & b</a>"),
            "&lt;a href=&quot;x&quot; title=&#39;y&#39;&gt;a &amp; b&lt;/a&gt;"
        )
    }

    #[test]
    fn leaves_plain_text_untouched() {
        assert_eq!(escape_html("hello world"), "hello world")
    }

    #[test]
    fn does_not_double_escape_entities() {
        assert_eq!(escape_html("&amp;"), "&amp;amp;")
    }
}
