//! Serialized HTML is a Rust `String`, so it is UTF-8 whatever charset the page was crawled in.
//! A page decoded from windows-1251 at ingest still carries `<meta charset=windows-1251>`, and a
//! browser opening the written file would believe the label:
//!
//! ```text
//! stored:  <meta charset="windows-1251"><p>Привет</p>
//! written: <meta charset="utf-8"><p>Привет</p>   (with the old label: РџСЂРёРІРµС‚)
//! ```
//!
//! So the serializer writes every `<meta>` charset label as `utf-8`, as a browser's "Save page"
//! does. The stored attribute keeps the page's own label, so `meta[charset]` still finds it.

use std::borrow::Cow;

use crate::html::HtmlAttr;
use crate::stores::{AttrName, Attribute};

/// True if a `<meta>`'s attributes include `http-equiv="Content-Type"`, whose `content` then
/// carries a charset label.
pub(crate) fn declares_content_type<'a>(mut attrs: impl Iterator<Item = Attribute<'a>>) -> bool {
    attrs.any(|a| {
        a.name == AttrName::Std(HtmlAttr::http_equiv)
            && a.val.trim().eq_ignore_ascii_case("content-type")
    })
}

/// The value to write for one attribute of a `<meta>`: its charset label replaced by `utf-8`.
pub(crate) fn utf8_value<'a>(attr: &Attribute<'a>, content_type: bool) -> Cow<'a, str> {
    let val = attr.val;
    let label = match attr.name {
        AttrName::Std(HtmlAttr::charset) => Some((0, val.len())),
        AttrName::Std(HtmlAttr::content) if content_type => charset_label(val),
        _ => None,
    };
    match label {
        Some((start, end)) if !val[start..end].trim().eq_ignore_ascii_case("utf-8") => {
            Cow::Owned(format!("{}utf-8{}", &val[..start], &val[end..]))
        }
        _ => Cow::Borrowed(val),
    }
}

/// The byte range of the label after `charset=` in a `content` value such as
/// `text/html; charset=windows-1251`, without quotes.
fn charset_label(val: &str) -> Option<(usize, usize)> {
    let bytes = val.as_bytes();
    let mut from = 0;
    loop {
        let at = from
            + bytes[from..]
                .windows(b"charset".len())
                .position(|w| w.eq_ignore_ascii_case(b"charset"))?;
        from = at + b"charset".len();
        let mut i = from;
        while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        if bytes.get(i) != Some(&b'=') {
            continue;
        }
        i += 1;
        while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        let quote = bytes.get(i).copied().filter(|&b| b == b'"' || b == b'\'');
        let start = i + usize::from(quote.is_some());
        let len = bytes[start..]
            .iter()
            .position(|&b| match quote {
                Some(q) => b == q,
                None => b == b';' || b == b'"' || b == b'\'' || b.is_ascii_whitespace(),
            })
            .unwrap_or(bytes.len() - start);
        return Some((start, start + len));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written(attr_name: HtmlAttr, val: &str, content_type: bool) -> String {
        let attr = Attribute {
            name: AttrName::Std(attr_name),
            val,
        };
        utf8_value(&attr, content_type).into_owned()
    }

    #[test]
    fn charset_attribute_is_written_as_utf8() {
        assert_eq!(written(HtmlAttr::charset, "windows-1251", false), "utf-8");
        assert_eq!(written(HtmlAttr::charset, "UTF-8", false), "UTF-8");
    }

    #[test]
    fn content_type_label_is_replaced_in_place() {
        let cases = [
            (
                "text/html; charset=windows-1251",
                "text/html; charset=utf-8",
            ),
            (
                "text/html;charset = 'Shift_JIS' ",
                "text/html;charset = 'utf-8' ",
            ),
            (
                "text/html; charset=EUC-JP; x=y",
                "text/html; charset=utf-8; x=y",
            ),
            ("text/html", "text/html"),
        ];
        for (stored, want) in cases {
            assert_eq!(written(HtmlAttr::content, stored, true), want, "{stored}");
        }
    }

    /// Without `http-equiv=Content-Type`, `content` is prose, such as a description.
    #[test]
    fn other_content_is_left_alone() {
        let text = "How to set charset=gbk in PHP";
        assert_eq!(written(HtmlAttr::content, text, false), text);
    }
}
