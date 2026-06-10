//! Entity codec for htmlarc's decoded text/attribute storage: decode on ingest,
//! re-encode on serialize.
//!
//! Decoding happens once, at the parse→store boundary (regular text, RCDATA
//! `title`/`textarea`, attribute values, data-attribute values). `script`/`style`
//! RAWTEXT and comments are left verbatim. Serialization re-encodes text-node content
//! and attribute values.
//!
//! The named/numeric tables come from the `html-escape` crate — the interim source
//! pending the tokenizer decision. An html5gum switch would supply its own tables
//! here; the call sites across the crate stay unchanged.
use std::borrow::Cow;

/// Decode HTML character references (named + numeric) to their characters.
/// Zero-copy (`Borrowed`) when the input contains no entity.
pub(crate) fn decode(s: &str) -> Cow<'_, str> {
    html_escape::decode_html_entities(s)
}

/// Escape text-node content for serialization (`&`, `<`, `>`).
pub(crate) fn encode_text(s: &str) -> Cow<'_, str> {
    html_escape::encode_text(s)
}

/// Escape a double-quoted attribute value for serialization (`&`, `"`).
pub(crate) fn encode_attr(s: &str) -> Cow<'_, str> {
    html_escape::encode_double_quoted_attribute(s)
}

#[cfg(test)]
mod tests {
    use crate::fmt::HtmlFormat;
    use crate::html::HtmlDoc;

    /// parse -> serialize (Raw, the byte-oriented format).
    fn rt(html: &str) -> String {
        HtmlDoc::parse(html).unwrap().to_html(HtmlFormat::Raw)
    }

    /// The pipeline must reach a FIXED POINT: serializing the output again changes
    /// nothing. This is the key robustness guarantee — repeated pack/serialize cycles
    /// can never progressively corrupt content (the failure mode of naive decode/encode).
    #[track_caller]
    fn assert_fixed_point(html: &str) {
        let once = rt(html);
        let twice = rt(&once);
        assert_eq!(once, twice, "not a fixed point for: {html:?}");
    }

    #[test]
    fn self_encoding_entities_round_trip_byte_identical() {
        // `&` `<` `>` (text) and `&` `"` (attrs) re-encode to exactly themselves.
        for s in [
            "<p>a &amp; b</p>",
            "<p>a &lt; b &gt; c</p>",
            r#"<a href="/p?a=1&amp;b=2">x</a>"#,
            r#"<a title="say &quot;hi&quot;">x</a>"#,
        ] {
            assert_eq!(rt(s), s, "must round-trip byte-identical: {s:?}");
        }
    }

    #[test]
    fn named_numeric_and_hex_entities_decode_to_characters() {
        // All three spellings of U+00A0 collapse to the real character.
        assert_eq!(rt("<p>a&nbsp;b</p>"), "<p>a\u{a0}b</p>");
        assert_eq!(rt("<p>a&#160;b</p>"), "<p>a\u{a0}b</p>");
        assert_eq!(rt("<p>a&#xA0;b</p>"), "<p>a\u{a0}b</p>");
        assert_eq!(rt("<p>&mdash;</p>"), "<p>\u{2014}</p>");
        assert_eq!(rt("<p>&copy;</p>"), "<p>\u{a9}</p>");
    }

    #[test]
    fn numeric_ampersand_canonicalizes_to_amp() {
        // &#38; and &#x26; decode to '&', which re-encodes to the canonical &amp;.
        assert_eq!(rt("<p>&#38;</p>"), "<p>&amp;</p>");
        assert_eq!(rt("<p>&#x26;</p>"), "<p>&amp;</p>");
    }

    #[test]
    fn bare_ampersand_and_nested_entity_are_stable() {
        // A bare '&' survives, then encodes to &amp; — and stays put thereafter.
        assert_eq!(rt("<p>Tom & Jerry</p>"), "<p>Tom &amp; Jerry</p>");
        assert_fixed_point("<p>Tom & Jerry</p>");
        // &amp;amp; decodes one level (-> &amp;) and re-encodes back to &amp;amp;.
        assert_eq!(rt("<p>&amp;amp;</p>"), "<p>&amp;amp;</p>");
        assert_fixed_point("<p>&amp;amp;</p>");
    }

    #[test]
    fn rawtext_script_and_style_are_never_touched() {
        // RAWTEXT content must pass through verbatim — encoding it would corrupt JS/CSS,
        // and decoding `&amp;` inside a script would change program text.
        for s in [
            r#"<script>if (a && b < c) { x = "&amp;"; }</script>"#,
            r#"<style>a::before{content:"&"} /* a && b < c */</style>"#,
        ] {
            assert_eq!(rt(s), s, "RAWTEXT must be verbatim: {s:?}");
        }
    }

    #[test]
    fn comments_are_not_decoded() {
        let c = "<!-- a &amp; b < c -->";
        assert_eq!(rt(c), c);
    }

    #[test]
    fn rcdata_title_and_textarea_decode_then_reencode() {
        // RCDATA elements DO decode entities (unlike RAWTEXT script/style).
        assert_eq!(rt("<title>a &amp; b</title>"), "<title>a &amp; b</title>");
        assert_eq!(
            rt("<textarea>x &lt; y</textarea>"),
            "<textarea>x &lt; y</textarea>"
        );
    }

    #[test]
    fn data_attribute_values_decode() {
        assert_eq!(
            rt(r#"<div data-x="a&amp;b"></div>"#),
            r#"<div data-x="a&amp;b"></div>"#
        );
    }

    #[test]
    fn fixed_point_holds_on_dense_mixed_entities() {
        // Everything at once — attrs, text, RCDATA, RAWTEXT, comment — still converges.
        assert_fixed_point(
            r#"<a href="/p?x=1&amp;y=2" title="say &quot;hi&quot;">Tom &amp; Jerry &mdash; done&nbsp;now &lt;ok&gt;</a><script>a && b</script><!-- c & d -->"#,
        );
    }

    #[test]
    fn edge_cases() {
        assert_eq!(rt("<p></p>"), "<p></p>"); // empty element
        assert_eq!(rt("<p>&amp;</p>"), "<p>&amp;</p>"); // value is only an entity
        assert_fixed_point("<p>&amp;</p>");
        // Entity split across the very start/end of a text run.
        assert_eq!(rt("<p>&amp;tail</p>"), "<p>&amp;tail</p>");
        assert_eq!(rt("<p>head&amp;</p>"), "<p>head&amp;</p>");
    }
}
