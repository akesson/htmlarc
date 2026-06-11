use std::fmt::Display;

use strum_macros::{EnumString, FromRepr, IntoStaticStr};

/// An html tag is any valid html tag. Note that any node that starts with sys_ is not a
/// valid html tag, and is used by the system.
#[allow(non_camel_case_types, clippy::upper_case_acronyms)]
#[repr(u8)]
#[derive(FromRepr, EnumString, IntoStaticStr, Debug, PartialEq, Eq, Clone, Copy, Default, Hash)]
#[strum(ascii_case_insensitive)]
pub enum HtmlTag {
    #[default]
    sys_root = 0,
    sys_deleted,
    #[strum(serialize = "text")]
    sys_text,
    #[strum(serialize = "comment")]
    sys_comment,
    DOCTYPE,
    a,
    abbr,
    acronym,
    address,
    applet,
    area,
    article,
    aside,
    audio,
    b,
    base,
    basefont,
    bdi,
    bdo,
    big,
    blockquote,
    body,
    br,
    button,
    canvas,
    caption,
    center,
    cite,
    code,
    col,
    colgroup,
    data,
    datalist,
    dd,
    del,
    details,
    dfn,
    dialog,
    dir,
    div,
    dl,
    dt,
    em,
    embed,
    fieldset,
    figcaption,
    figure,
    font,
    footer,
    form,
    frame,
    frameset,
    h1,
    h2,
    h3,
    h4,
    h5,
    h6,
    head,
    header,
    hgroup,
    hr,
    html,
    i,
    iframe,
    img,
    input,
    ins,
    kbd,
    label,
    legend,
    li,
    link,
    main,
    map,
    math,
    mark,
    menu,
    meta,
    meter,
    nav,
    noframes,
    noscript,
    object,
    ol,
    optgroup,
    option,
    output,
    p,
    param,
    picture,
    pre,
    progress,
    q,
    rp,
    rb,
    rt,
    ruby,
    s,
    samp,
    script,
    search,
    section,
    select,
    small,
    source,
    span,
    strike,
    strong,
    style,
    sub,
    summary,
    sup,
    svg,
    table,
    tbody,
    td,
    template,
    textarea,
    tfoot,
    th,
    thead,
    time,
    title,
    tr,
    track,
    tt,
    u,
    ul,
    var,
    video,
    wbr,
    /// Normalization marker for an *extended* (custom/unknown) tag — any name not in this
    /// enum (ADR 0002 §4). It is the in-memory result of [`crate::dom::nodes`] decoding a
    /// node byte `>= EXT_BASE`; the real name lives in the per-document `ext_tags` vocab and
    /// is resolved via `DomView::tag_name`. It is **never stored as a node's own
    /// discriminant** (a node byte is either a sub-`EXT_BASE` discriminant or a vocab index)
    /// and is intentionally absent from every classifier below, so the default arms apply:
    /// extended elements are non-void, non-raw-text, non-inline/block, and never auto-close.
    /// Must remain the LAST variant so `from_repr(extended as u8 + 1)` is `None`.
    extended,
}

impl HtmlTag {
    pub fn as_str(&self) -> &'static str {
        self.into()
    }

    /// Parse a *tag name* — from a start/end tag or a CSS type selector — into a real HTML
    /// element. Returns `None` both for unknown names (custom elements) and for the reserved
    /// system/normalization spellings (`text`, `comment`, `doctype`, `extended`, …) that
    /// strum's case-insensitive `FromStr` would otherwise alias onto a non-element variant.
    /// Those become *extended* tags, so `<text>` round-trips as a custom element rather than a
    /// malformed system node, and `extended` can never be conjured from a name (ADR 0002 §4).
    /// `<!DOCTYPE …>` is unaffected: the tokenizer emits it through its own doctype path, not
    /// this name lookup.
    pub(crate) fn from_tag_name(name: &str) -> Option<HtmlTag> {
        match HtmlTag::try_from(name) {
            Ok(tag) if tag.is_reserved_spelling() => None,
            Ok(tag) => Some(tag),
            Err(_) => None,
        }
    }

    /// Whether this variant's spelling names a system/normalization marker rather than a
    /// parseable HTML element (see [`from_tag_name`](Self::from_tag_name)).
    fn is_reserved_spelling(self) -> bool {
        matches!(
            self,
            HtmlTag::sys_root
                | HtmlTag::sys_deleted
                | HtmlTag::sys_text
                | HtmlTag::sys_comment
                | HtmlTag::DOCTYPE
                | HtmlTag::extended
        )
    }

    /// Raw text elements are elements with text/script content that
    /// might interfere with the normal html syntax
    pub fn is_raw_text(&self) -> bool {
        matches!(
            self,
            HtmlTag::script | HtmlTag::style | HtmlTag::textarea | HtmlTag::title
        )
    }

    /// if the start tag should end with a closing />
    pub fn auto_close(&self) -> bool {
        use HtmlTag::{br, hr, img, input};
        // there should be a check for foreign elements as well, but it is
        // not necessary since they are discarded
        matches!(self, hr | br | img | input)
    }

    /// if the element has a closing tag
    pub fn no_close(&self) -> bool {
        use HtmlTag::{DOCTYPE, sys_comment, sys_text};
        matches!(self, sys_text | sys_comment | DOCTYPE) || self.is_void_element()
    }

    pub fn is_void_element(&self) -> bool {
        matches!(
            self,
            HtmlTag::area
                | HtmlTag::base
                | HtmlTag::br
                | HtmlTag::col
                | HtmlTag::embed
                | HtmlTag::hr
                | HtmlTag::img
                | HtmlTag::input
                | HtmlTag::link
                | HtmlTag::meta
                | HtmlTag::param
                // | HtmlTag::source
                | HtmlTag::track
                | HtmlTag::wbr
        )
    }

    pub fn is_foreign_element(&self) -> bool {
        matches!(self, HtmlTag::math | HtmlTag::svg)
    }

    pub fn is_format_inlined(&self) -> bool {
        matches!(
            self,
            HtmlTag::i
                | HtmlTag::b
                | HtmlTag::em
                | HtmlTag::small
                | HtmlTag::big
                | HtmlTag::sup
                | HtmlTag::sub
                | HtmlTag::q
                | HtmlTag::s
                | HtmlTag::strong
                | HtmlTag::summary
        )
    }

    pub fn is_inline_element(&self) -> bool {
        matches!(
            self,
            HtmlTag::a
                | HtmlTag::abbr
                | HtmlTag::acronym
                | HtmlTag::b
                | HtmlTag::bdo
                | HtmlTag::big
                | HtmlTag::br
                | HtmlTag::button
                | HtmlTag::cite
                | HtmlTag::code
                | HtmlTag::dfn
                | HtmlTag::em
                | HtmlTag::i
                | HtmlTag::img
                | HtmlTag::input
                | HtmlTag::kbd
                | HtmlTag::label
                | HtmlTag::map
                | HtmlTag::object
                | HtmlTag::output
                | HtmlTag::q
                | HtmlTag::samp
                | HtmlTag::script
                | HtmlTag::select
                | HtmlTag::small
                | HtmlTag::span
                | HtmlTag::strong
                | HtmlTag::sub
                | HtmlTag::sup
                | HtmlTag::textarea
                | HtmlTag::time
                | HtmlTag::tt
                | HtmlTag::var
                | HtmlTag::sys_text
        )
    }

    pub fn is_block_element(&self) -> bool {
        matches!(
            self,
            HtmlTag::address
                | HtmlTag::article
                | HtmlTag::aside
                | HtmlTag::blockquote
                | HtmlTag::canvas
                | HtmlTag::dd
                | HtmlTag::div
                | HtmlTag::dl
                | HtmlTag::dt
                | HtmlTag::fieldset
                | HtmlTag::figcaption
                | HtmlTag::figure
                | HtmlTag::footer
                | HtmlTag::form
                | HtmlTag::h1
                | HtmlTag::h2
                | HtmlTag::h3
                | HtmlTag::h4
                | HtmlTag::h5
                | HtmlTag::h6
                | HtmlTag::header
                | HtmlTag::hr
                | HtmlTag::li
                | HtmlTag::main
                | HtmlTag::nav
                | HtmlTag::noscript
                | HtmlTag::ol
                | HtmlTag::p
                | HtmlTag::pre
                | HtmlTag::section
                | HtmlTag::table
                | HtmlTag::tfoot
                | HtmlTag::ul
                | HtmlTag::video
        )
    }

    /// see: https://html.spec.whatwg.org/multipage/syntax.html#optional-tags
    pub fn auto_close_when_parent(&self, parent: Self) -> bool {
        use HtmlTag::{
            datalist, dd, dl, li, menu, ol, optgroup, option, rp, rt, ruby, select, table, tbody,
            td, tfoot, th, thead, tr, ul,
        };
        match self {
            option => matches!(parent, select | optgroup | datalist),
            li => matches!(parent, ol | ul | menu),
            dd => parent == dl,
            rt | rp => parent == ruby,
            optgroup => parent == select,
            tbody | tfoot => parent == table,
            tr => matches!(parent, table | thead | tbody),
            td | th => parent == tr,
            _ => false,
        }
    }

    /// see: https://html.spec.whatwg.org/multipage/syntax.html#optional-tags
    pub fn auto_close_when_next(&self, next: Self) -> bool {
        use HtmlTag::{
            colgroup, dd, dt, hr, li, optgroup, option, rp, rt, tbody, td, tfoot, th, thead, tr,
        };
        match self {
            li => next == li,
            dt | dd => matches!(next, dt | dd),
            rt | rp => matches!(next, rp | rt),
            optgroup => matches!(next, optgroup | hr),
            option => matches!(next, option | hr),
            colgroup => matches!(next, colgroup), // simplified
            thead => matches!(next, tbody | thead),
            tbody => matches!(next, tbody | tfoot), // simplified
            tr => matches!(next, tr),
            td | th => matches!(next, td | th),
            _ => false,
        }
    }
}

impl Display for HtmlTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PartialEq<&str> for HtmlTag {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl IntoIterator for HtmlTag {
    type Item = HtmlTag;
    type IntoIter = std::iter::Once<HtmlTag>;

    fn into_iter(self) -> Self::IntoIter {
        std::iter::once(self)
    }
}
