/// From reference/packages/core/src/util/token.ts
///
/// The reference estimates tokens by dividing the UTF-16 code unit length by
/// four: `Math.max(0, Math.round(input.length / CHARS_PER_TOKEN))`.
pub fn estimate(input: &str) -> u64 {
    let units = input.encode_utf16().count() as f64;
    (units / 4.0).round().max(0.0) as u64
}

#[cfg(test)]
mod tests {
    use super::estimate;

    #[test]
    fn empty_string_is_zero() {
        assert_eq!(estimate(""), 0);
    }

    #[test]
    fn rounds_to_nearest_token() {
        assert_eq!(estimate("abcd"), 1);
        assert_eq!(estimate("abcdef"), 2);
        assert_eq!(estimate("abc"), 1);
        assert_eq!(estimate("ab"), 1);
    }

    #[test]
    fn counts_utf16_units() {
        assert_eq!(estimate(&"a".repeat(10)), 3);
        assert_eq!(estimate("😀"), 1);
    }
}
