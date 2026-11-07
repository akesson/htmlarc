//! Decoding raw HTML bytes to UTF-8 at ingest.
//!
//! Crawled pages arrive in whatever charset the server used, and the parser takes `&str`, so a
//! page has to be decoded exactly once, here. The order differs from a browser's on purpose: a
//! browser trusts the label, but crawl labels are often wrong (Apache's default header calling
//! UTF-8 pages ISO-8859-1), so the bytes are tried as UTF-8 before any label is believed.
//!
//! ```text
//! caf\xE9, Content-Type: text/html; charset=windows-1252   -> "café"
//! caf\xC3\xA9, Content-Type: text/html; charset=iso-8859-1 -> "café"  (valid UTF-8 wins)
//! ```

use std::borrow::Cow;

use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};
use encoding_rs::{Encoding, UTF_8, WINDOWS_1252, X_USER_DEFINED};

/// How far into a document to look for `<meta charset>`. The standard's prescan stops at 1,024
/// bytes; real pages put more before it, and past the window a page falls through to detection.
const META_PRESCAN: usize = 4096;

/// Valid multi-byte characters a page needs per invalid sequence to count as damaged UTF-8. On a
/// Common Crawl segment, pages with a legacy label stay below 4 and mislabelled UTF-8 sits above
/// 64.
const VALID_PER_INVALID: usize = 4;

/// The same ratio for a page whose highest-ranked label says UTF-8. The label and the valid
/// characters agree, so a page needs only as many valid characters as invalid sequences: three
/// curly quotes and one stray `\x92` stay UTF-8. Legacy bytes rarely form valid UTF-8, so a
/// mislabelled legacy page still has almost no valid characters.
const VALID_PER_INVALID_UTF8_LABEL: usize = 1;

/// Decode `bytes`. `content_type` is the HTTP header's value and `url` the page's address, when
/// the source has them.
pub(crate) fn decode_html<'a>(
    bytes: &'a [u8],
    content_type: Option<&[u8]>,
    url: Option<&str>,
) -> Cow<'a, str> {
    if let Some((encoding, bom_len)) = Encoding::for_bom(bytes) {
        return encoding.decode_without_bom_handling(&bytes[bom_len..]).0;
    }
    // `from_utf8` has an ASCII fast path the byte-at-a-time lossy decoder lacks, and nearly every
    // page ends here.
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Cow::Borrowed(text);
    }
    choose_encoding(bytes, content_type, url)
        .decode_without_bom_handling(bytes)
        .0
}

/// The encoding of a page that is not valid UTF-8.
fn choose_encoding(
    bytes: &[u8],
    content_type: Option<&[u8]>,
    url: Option<&str>,
) -> &'static Encoding {
    // The header's label, then each `<meta>`'s, in document order.
    let labels: Vec<&'static Encoding> = content_type
        .and_then(charset_label)
        .and_then(label_encoding)
        .into_iter()
        .chain(meta_encodings(bytes))
        .collect();
    // Only the label that would otherwise win lowers the bar: a template's `<meta charset=utf-8>`
    // under a `windows-1251` header doesn't make the header's page UTF-8.
    if is_damaged_utf8(bytes, labels.first() == Some(&UTF_8)) {
        return UTF_8;
    }
    // The bytes just disproved any UTF-8 label, so it says nothing; the next one might.
    labels
        .into_iter()
        .find(|&e| e != UTF_8)
        .unwrap_or_else(|| detect(bytes, url))
}

/// True for UTF-8 with a few bad bytes, as left by truncating a record or pasting in a fragment.
/// Decoding such a page as a legacy charset would garble all of its text to save a few bytes.
fn is_damaged_utf8(bytes: &[u8], utf8_label: bool) -> bool {
    let per_invalid = if utf8_label {
        VALID_PER_INVALID_UTF8_LABEL
    } else {
        VALID_PER_INVALID
    };
    let (mut valid, mut invalid) = (0, 0);
    let mut chunks = bytes.utf8_chunks().peekable();
    while let Some(chunk) = chunks.next() {
        valid += chunk.valid().chars().filter(|c| !c.is_ascii()).count();
        let bad = chunk.invalid();
        // A character cut off at the very end is what truncation leaves, not evidence. A stray
        // byte there still counts.
        let cut_off = chunks.peek().is_none()
            && std::str::from_utf8(bad).is_err_and(|e| e.error_len().is_none());
        if !bad.is_empty() && !cut_off {
            invalid += 1;
        }
    }
    // With no valid character, only a UTF-8 label is evidence for UTF-8: unlabelled, `caf\xE9` is
    // a Latin-1 word rather than a truncated page.
    (valid > 0 || utf8_label) && valid >= invalid * per_invalid
}

/// The encoding a charset label names, as this module uses it.
///
/// - Encodings that aren't ASCII-compatible (the UTF-16s) are refused. The page has already
///   failed as UTF-8, so a `charset=utf-16` header is far likelier to be wrong about an 8-bit
///   page than right about a UTF-16 one without a BOM, and believing it would turn every tag into
///   CJK-looking text. In `<meta>`, a UTF-16 page couldn't have spelled the label in ASCII anyway.
/// - `x-user-defined` means windows-1252, as in a browser's prescan. Its own decoder maps every
///   high byte to the Private Use Area: `caf\xE9` would become `caf\u{F7E9}`.
/// - Replacement-only labels such as `iso-2022-kr` are unknown, because they would blank the page.
fn label_encoding(label: &[u8]) -> Option<&'static Encoding> {
    match Encoding::for_label_no_replacement(label)? {
        e if e == X_USER_DEFINED => Some(WINDOWS_1252),
        e => e.is_ascii_compatible().then_some(e),
    }
}

/// The encodings named by `<meta>` tags in the prescan window, in document order.
fn meta_encodings(bytes: &[u8]) -> impl Iterator<Item = &'static Encoding> + '_ {
    let mut rest = &bytes[..bytes.len().min(META_PRESCAN)];
    std::iter::from_fn(move || {
        loop {
            rest = &rest[rest.iter().position(|&b| b == b'<')?..];
            if rest.starts_with(b"<!--") {
                rest = &rest[find_ci(rest, b"-->")? + 3..];
            } else if starts_with_ci(rest, b"<meta")
                // As in the standard's prescan: `<meta` then a space, `/` or `>`, not `<metadata`.
                && rest
                    .get(5)
                    .is_some_and(|&b| b.is_ascii_whitespace() || b == b'/' || b == b'>')
            {
                let end = rest.iter().position(|&b| b == b'>')?;
                let tag = &rest[..end];
                rest = &rest[end..];
                if let Some(encoding) = charset_label(tag).and_then(label_encoding) {
                    return Some(encoding);
                }
            } else {
                rest = &rest[1..];
            }
        }
    })
}

/// Guess the charset of a page that declares none. UTF-8 is denied because the caller has ruled
/// it out. The top-level domain raised agreement with declared labels from 91% to 93%.
fn detect(bytes: &[u8], url: Option<&str>) -> &'static Encoding {
    let mut detector = EncodingDetector::new(Iso2022JpDetection::Deny);
    detector.feed(bytes, true);
    let tld = url.and_then(top_level_domain);
    detector.guess(tld.as_deref().map(str::as_bytes), Utf8Detection::Deny)
}

/// The rightmost host label, lower-cased. Anything non-ASCII yields `None`, because
/// [`EncodingDetector::guess`] panics on it.
fn top_level_domain(url: &str) -> Option<String> {
    let authority = url.split_once("://")?.1.split(['/', '?', '#']).next()?;
    let host = authority.rsplit('@').next()?.split(':').next()?;
    // A fully qualified `example.ru.` ends in an empty label; the TLD is the one before it.
    let host = host.strip_suffix('.').unwrap_or(host);
    let label = host.rsplit('.').next()?.to_ascii_lowercase();
    (!label.is_empty() && label.is_ascii()).then_some(label)
}

/// The value after `charset=`, unquoted.
fn charset_label(s: &[u8]) -> Option<&[u8]> {
    let mut rest = s;
    loop {
        rest = &rest[find_ci(rest, b"charset")? + b"charset".len()..];
        let after_ws = rest.trim_ascii_start();
        let Some(value) = after_ws.strip_prefix(b"=") else {
            continue;
        };
        let value = value.trim_ascii_start();
        let (quote, value) = match value.first() {
            Some(&q @ (b'"' | b'\'')) => (Some(q), &value[1..]),
            _ => (None, value),
        };
        let end = value
            .iter()
            .position(|&b| match quote {
                Some(q) => b == q,
                None => b == b';' || b == b'"' || b == b'\'' || b.is_ascii_whitespace(),
            })
            .unwrap_or(value.len());
        return Some(&value[..end]);
    }
}

pub(super) fn starts_with_ci(s: &[u8], prefix: &[u8]) -> bool {
    s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix)
}

pub(super) fn find_ci(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HTML_1251: &[u8] = b"<p>\xcf\xf0\xe8\xe2\xe5\xf2 \xec\xe8\xf0</p>"; // Привет мир
    const HTML_SJIS: &[u8] = b"<p>\x93\xfa\x96\x7b\x8c\xea</p>"; // 日本語

    fn decoded(bytes: &[u8], content_type: Option<&str>) -> String {
        decode_html(bytes, content_type.map(str::as_bytes), None).into_owned()
    }

    #[test]
    fn valid_utf8_borrows_and_ignores_a_wrong_label() {
        let html = "<p>caf\u{e9}</p>".as_bytes();
        let text = decode_html(html, Some(b"text/html; charset=iso-8859-1"), None);
        assert!(matches!(text, Cow::Borrowed(_)));
        assert_eq!(text, "<p>caf\u{e9}</p>");
    }

    #[test]
    fn header_label_decodes_legacy_bytes() {
        let ct = Some("text/html; charset=windows-1251");
        assert_eq!(
            decoded(HTML_1251, ct),
            "<p>\u{41f}\u{440}\u{438}\u{432}\u{435}\u{442} \u{43c}\u{438}\u{440}</p>"
        );
    }

    #[test]
    fn header_label_is_parsed_in_its_usual_spellings() {
        for ct in [
            "text/html; charset=Shift_JIS",
            "text/html;charset=\"shift_jis\"",
            "TEXT/HTML; CHARSET = 'SJIS' ; foo=bar",
        ] {
            assert_eq!(
                decoded(HTML_SJIS, Some(ct)),
                "<p>\u{65e5}\u{672c}\u{8a9e}</p>",
                "{ct}"
            );
        }
    }

    #[test]
    fn meta_label_is_used_without_a_header() {
        for meta in [
            "<meta charset=shift_jis>",
            "<META CHARSET=\"Shift_JIS\">",
            "<meta http-equiv=\"Content-Type\" content=\"text/html; charset=shift_jis\">",
        ] {
            let page = [meta.as_bytes(), HTML_SJIS].concat();
            assert!(
                decoded(&page, None).ends_with("<p>\u{65e5}\u{672c}\u{8a9e}</p>"),
                "{meta}"
            );
        }
    }

    #[test]
    fn meta_in_a_comment_is_ignored() {
        let page = [
            b"<!-- <meta charset=koi8-r> --><meta charset=shift_jis>",
            HTML_SJIS,
        ]
        .concat();
        assert_eq!(meta_encodings(&page).next(), Some(encoding_rs::SHIFT_JIS));
    }

    #[test]
    fn meta_past_the_prescan_window_is_not_seen() {
        let page = [
            vec![b' '; META_PRESCAN].as_slice(),
            b"<meta charset=shift_jis>",
        ]
        .concat();
        assert_eq!(meta_encodings(&page).next(), None);
    }

    #[test]
    fn meta_cannot_claim_utf16() {
        assert_eq!(meta_encodings(b"<meta charset=utf-16>").next(), None);
    }

    /// A UTF-16 header on an 8-bit page would turn every tag into CJK-looking text.
    #[test]
    fn a_utf16_header_is_refused() {
        assert_eq!(
            decoded(b"<p>caf\xe9</p>", Some("text/html; charset=utf-16")),
            "<p>caf\u{e9}</p>"
        );
    }

    /// `x-user-defined` would map every high byte to the Private Use Area.
    #[test]
    fn x_user_defined_means_windows_1252() {
        let page = b"<meta charset=x-user-defined><p>caf\xe9</p>";
        assert!(decoded(page, None).ends_with("<p>caf\u{e9}</p>"));
        let ct = Some("text/html; charset=x-user-defined");
        assert_eq!(decoded(b"<p>caf\xe9</p>", ct), "<p>caf\u{e9}</p>");
    }

    /// Templates often emit `<meta charset=utf-8>` and then the real legacy label.
    #[test]
    fn a_disproved_utf8_meta_defers_to_the_next_meta() {
        let page = [
            b"<meta charset=utf-8><meta http-equiv=Content-Type content=\"text/html; charset=koi8-r\">"
                .as_slice(),
            b"<p>\xf0\xd2\xc9\xd7\xc5\xd4</p>", // Привет
        ]
        .concat();
        assert_eq!(meta_encodings(&page).nth(1), Some(encoding_rs::KOI8_R));
        assert!(
            decoded(&page, None).ends_with("<p>\u{41f}\u{440}\u{438}\u{432}\u{435}\u{442}</p>")
        );
    }

    #[test]
    fn a_disproved_utf8_header_defers_to_meta() {
        let page = [b"<meta charset=shift_jis>", HTML_SJIS].concat();
        let text = decoded(&page, Some("text/html; charset=utf-8"));
        assert!(text.ends_with("<p>\u{65e5}\u{672c}\u{8a9e}</p>"));
    }

    #[test]
    fn an_unknown_label_falls_through_to_detection() {
        // `none` occurs in real pages. Replacement-only labels must not blank the page either.
        for ct in ["text/html; charset=none", "text/html; charset=iso-2022-kr"] {
            let page = "\u{41f}\u{440}\u{438}\u{432}\u{435}\u{442} ".repeat(40);
            let (bytes, _, _) = encoding_rs::WINDOWS_1251.encode(&page);
            assert_eq!(decoded(&bytes, Some(ct)), page, "{ct}");
        }
    }

    #[test]
    fn utf8_with_a_few_bad_bytes_stays_utf8_despite_a_legacy_label() {
        let good = "\u{65e5}\u{672c}\u{8a9e}".repeat(10);
        let page = [good.as_bytes(), b"\xff", good.as_bytes()].concat();
        let text = decoded(&page, Some("text/html; charset=windows-1251"));
        assert_eq!(text, format!("{good}\u{fffd}{good}"));
    }

    /// With a UTF-8 label, one valid character per bad byte is enough. Without the label this
    /// page would go to the detector and every curly quote would turn into `â€œ`.
    #[test]
    fn utf8_labelled_page_with_a_stray_byte_stays_utf8() {
        let page = [
            "<p>\u{201c}Hello\u{201d} it\u{2019}s ".as_bytes(),
            b"don\x92t</p>",
        ]
        .concat();
        let want = "<p>\u{201c}Hello\u{201d} it\u{2019}s don\u{fffd}t</p>";
        assert_eq!(decoded(&page, Some("text/html; charset=utf-8")), want);
        let with_meta = [b"<meta charset=utf-8>".as_slice(), &page].concat();
        assert!(decoded(&with_meta, None).ends_with(want));
    }

    /// A template's `<meta charset=utf-8>` doesn't lower the bar under a legacy header, which
    /// outranks it. Had any UTF-8 label counted, 8 valid characters against 6 invalid sequences
    /// would have kept this page as UTF-8 and blanked its windows-1251 text.
    #[test]
    fn a_utf8_meta_under_a_legacy_header_keeps_the_strict_bar() {
        let page = [
            b"<meta charset=utf-8><p>".as_slice(),
            "\u{a9}".repeat(8).as_bytes(),
            b"\xcf\xf0\xe8\xe2\xe5\xf2 </p>", // Привет
        ]
        .concat();
        let text = decoded(&page, Some("text/html; charset=windows-1251"));
        assert!(
            text.ends_with("\u{41f}\u{440}\u{438}\u{432}\u{435}\u{442} </p>"),
            "{text}"
        );
    }

    #[test]
    fn metadata_is_not_a_meta_tag() {
        assert_eq!(meta_encodings(b"<metadata charset=koi8-r>").next(), None);
        assert_eq!(
            meta_encodings(b"<meta/charset=koi8-r>").next(),
            Some(encoding_rs::KOI8_R)
        );
    }

    /// With a UTF-8 label, an ASCII page cut off mid-character is UTF-8, not a detector guess.
    #[test]
    fn utf8_labelled_ascii_cut_off_mid_character_stays_utf8() {
        let text = decoded(b"<p>abc\xe6\x97", Some("text/html; charset=utf-8"));
        assert_eq!(text, "<p>abc\u{fffd}");
    }

    /// Only a character cut off at the end is exempt; a stray byte there still counts.
    #[test]
    fn a_stray_trailing_byte_is_evidence() {
        assert!(!is_damaged_utf8(b"\xc3\xa9\xff", false)); // 1 valid, 1 invalid
        assert!(is_damaged_utf8(b"\xc3\xa9\xe6\x97", false)); // 1 valid, cut-off tail
    }

    #[test]
    fn utf8_truncated_mid_character_stays_utf8() {
        let page = "caf\u{e9} \u{65e5}".as_bytes();
        let cut = &page[..page.len() - 1];
        let text = decoded(cut, Some("text/html; charset=windows-1252"));
        assert_eq!(text, "caf\u{e9} \u{fffd}");
    }

    #[test]
    fn a_bom_beats_every_label_and_is_removed() {
        assert_eq!(
            decoded(b"\xef\xbb\xbf<p>", Some("text/html; charset=koi8-r")),
            "<p>"
        );
        assert_eq!(decoded(b"\xff\xfe<\0p\0>\0", None), "<p>");
    }

    #[test]
    fn the_domain_reaches_the_detector() {
        assert_eq!(
            top_level_domain("http://www.Example.CO.JP:8080/a?b").as_deref(),
            Some("jp")
        );
        assert_eq!(
            top_level_domain("https://user:pw@example.ru").as_deref(),
            Some("ru")
        );
        // A fully qualified name's trailing dot is not an empty TLD.
        assert_eq!(
            top_level_domain("http://example.ru./").as_deref(),
            Some("ru")
        );
    }

    /// The detector panics on these, so none may get through.
    #[test]
    fn hosts_the_detector_would_panic_on_are_dropped() {
        for url in [
            "http://caf\u{e9}.\u{f6}l/",
            "http://example.com../",
            "not a url",
            "http:///x",
        ] {
            let tld = top_level_domain(url);
            detect(b"\xe9", Some(url));
            assert_eq!(tld, None, "{url}");
        }
    }
}
