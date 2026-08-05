use oc_command::frontmatter::{parse_str, Markdown};
use serde_json::json;

fn data(md: &Markdown) -> &serde_json::Value {
    &md.data
}

#[test]
fn parses_simple_yaml_frontmatter() {
    let md = parse_str("---\nname: my-skill\ndescription: does things\n---\nbody text").unwrap();
    assert_eq!(
        data(&md),
        &json!({ "name": "my-skill", "description": "does things" })
    );
    assert_eq!(md.content, "body text");
}

#[test]
fn strips_only_one_newline_after_closing_delimiter() {
    // gray-matter drops a single `\n` immediately after the closing `---`;
    // an extra blank line is preserved.
    let md = parse_str("---\nname: foo\n---\n\nbody text\n").unwrap();
    assert_eq!(md.content, "\nbody text\n");
}

#[test]
fn no_frontmatter_returns_empty_data_and_full_content() {
    let md = parse_str("just some content\n").unwrap();
    assert_eq!(data(&md), &json!(null));
    assert_eq!(md.content, "just some content\n");
}

#[test]
fn extended_delimiter_is_not_frontmatter() {
    let md = parse_str("-----\nname: foo\n-----\nbody").unwrap();
    assert_eq!(data(&md), &json!(null));
    assert_eq!(md.content, "-----\nname: foo\n-----\nbody");
}

#[test]
fn empty_input_returns_empty_object() {
    let md = parse_str("").unwrap();
    assert_eq!(data(&md), &json!({}));
    assert_eq!(md.content, "");
}

#[test]
fn empty_frontmatter_returns_empty_object() {
    let md = parse_str("---\n---\ncontent here").unwrap();
    assert_eq!(data(&md), &json!({}));
    assert_eq!(md.content, "content here");
}

#[test]
fn comment_only_frontmatter_returns_empty_object() {
    let md = parse_str("---\n# just a comment\n---\nbody").unwrap();
    assert_eq!(data(&md), &json!({}));
    assert_eq!(md.content, "body");
}

#[test]
fn parses_crlf_frontmatter() {
    let md = parse_str("---\r\nname: foo\r\n---\r\nbody").unwrap();
    assert_eq!(data(&md), &json!({ "name": "foo" }));
    assert_eq!(md.content, "body");
}

#[test]
fn quoted_values_with_colons_parse() {
    let md = parse_str("---\ndescription: \"he said: hi\"\n---\nbody").unwrap();
    assert_eq!(data(&md), &json!({ "description": "he said: hi" }));
}

#[test]
fn boolean_and_string_types() {
    let md = parse_str("---\nname: foo\nsubtask: true\ncount: 5\n---\nbody").unwrap();
    assert_eq!(
        data(&md),
        &json!({ "name": "foo", "subtask": true, "count": 5 })
    );
}

#[test]
fn sanitize_fallback_handles_unquoted_colons() {
    let md = parse_str("---\ndescription: fix bug: now\n---\nbody").unwrap();
    assert_eq!(data(&md), &json!({ "description": "fix bug: now" }));
    assert_eq!(md.content, "body");
}

#[test]
fn sanitize_leaves_already_valid_frontmatter_unchanged() {
    let input = "---\ndescription: something valid\n---\nbody";
    assert_eq!(oc_command::frontmatter::sanitize(input), input);
}

#[test]
fn sanitize_skips_comment_and_indented_lines() {
    let input = "---\n# heading comment\ndescription: a: b\n---\nbody";
    let expected = "---\n# heading comment\ndescription: |-\n  a: b\n---\nbody";
    assert_eq!(oc_command::frontmatter::sanitize(input), expected);
}

#[test]
fn bare_scalar_frontmatter_is_parsed_as_string() {
    // js-yaml parses a plain scalar document as a string, not an error.
    let md = parse_str("---\njust bare words here\n---\nbody").unwrap();
    assert_eq!(data(&md), &json!("just bare words here"));
    assert_eq!(md.content, "body");
}

#[test]
fn invalid_flow_yaml_is_an_error() {
    assert!(parse_str("---\n{name: unclosed\n---\nbody").is_err());
}

#[test]
fn frontmatter_without_closing_delimiter_empties_content() {
    let md = parse_str("---\nname: foo\n").unwrap();
    assert_eq!(data(&md), &json!({ "name": "foo" }));
    assert_eq!(md.content, "");
}

#[test]
fn language_line_after_delimiter_is_consumed_as_language() {
    // gray-matter treats a non-empty first line after `---` as the parser
    // language and strips it from the frontmatter block.
    let md = parse_str("---yaml\nname: foo\n---\nbody").unwrap();
    assert_eq!(data(&md), &json!({ "name": "foo" }));
    assert_eq!(md.content, "body");
}

#[test]
fn golden_serialization_matches() {
    let md = parse_str("---\nname: my-skill\ndescription: does things\n---\nbody").unwrap();
    let serialized = serde_json::to_value(data(&md)).unwrap();
    assert_eq!(
        serialized,
        serde_json::from_str::<serde_json::Value>(
            "{\"name\":\"my-skill\",\"description\":\"does things\"}"
        )
        .unwrap()
    );
}
