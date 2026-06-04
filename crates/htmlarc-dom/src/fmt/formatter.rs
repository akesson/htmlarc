use std::cmp::Ordering;

use tinyvec::TinyVec;

use crate::{
    doc::{dom::DomRef, fmt::iter::Inliner, Element},
    HtmlTag,
};

use super::{
    fmt_buf::FmtBuf,
    format::Format,
    iter::{ElementInfo, TagStage},
};

#[derive(Clone, Default)]
struct FmtInfo {
    tag: HtmlTag,
    tabs: u8,
    depth: u16,
    is_preformatted: bool,
}

const DEFAULT_INFO: FmtInfo = FmtInfo {
    tag: HtmlTag::sys_root,
    tabs: 0,
    depth: 0,
    is_preformatted: false,
};

pub struct Formatter {
    pub format: Format,
    pub(crate) buf: FmtBuf,
    stack: TinyVec<[FmtInfo; 32]>,
}

impl Formatter {
    pub(crate) fn new(format: Format) -> Self {
        Self {
            format,
            buf: FmtBuf::default(),
            stack: TinyVec::new(),
        }
    }

    pub(crate) fn format<Dom: DomRef>(mut self, elem: Element<'_, Dom>) -> String {
        use super::super::element::ElementType::*;
        use TagStage::*;

        let inliner = Inliner::new(elem, self.format.inline.clone());

        for info in inliner {
            let el_type = info.element.get_type();
            match (info.stage, el_type) {
                (Open, Comment(comment)) => self.add_comment(info, comment),
                (Open, Text(text)) => self.add_text(info, text),
                (Open, Element(tag)) => self.open_tag(info, tag),
                (Close, Element(tag)) => self.close_tag(info, tag),
                (Close, Comment(_)) | (Close, Text(_)) => {}
            }
        }

        self.buf.inner()
    }

    fn with_prev_or<F: FnOnce(&FmtInfo) -> R, R>(&self, f: F, default: R) -> R {
        self.stack.last().map(f).unwrap_or(default)
    }

    fn add_text<Dom: DomRef>(&mut self, info: ElementInfo<'_, Dom>, text: &str) {
        if !info.in_inline_sequence {
            self.buf.newline();
        }

        self.buf
            .add_text(text, self.format.trim_text, info.in_inline_sequence);
    }

    fn add_comment<Dom: DomRef>(&mut self, _info: ElementInfo<'_, Dom>, comment: &str) {
        // put comments on a new line
        self.buf.newline();
        self.buf.add_coment(comment);
    }

    fn close_tag<Dom: DomRef>(&mut self, info: ElementInfo<'_, Dom>, _tag: HtmlTag) {
        let close = self.stack.pop().unwrap();

        // Return if we shouldn't close tags (inline elements are always closed)
        if !self.format.close_tags && !info.in_inline_sequence {
            return;
        }

        // Indent only if not in an inline sequence
        if !info.in_inline_sequence {
            self.buf.newline_and_indent(close.tabs);
        }

        if self.format.use_angular_brackets {
            self.buf.push_str("</");
            self.buf.push_str(close.tag.into());
            self.buf.push('>');
        } else {
            self.buf.push_str(close.tag.into());
        }
    }

    /// Any sequence of nodes that are inlineable should start and end with \n
    /// a node is inlineable if it is text, comment or element of type
    /// \<i\>, \<b\> etc. with all descendants inlineable.
    /// The last child of a node's children need to be indented
    /// because of the closing tag.

    fn open_tag<Dom: DomRef>(&mut self, info: ElementInfo<'_, Dom>, tag: HtmlTag) {
        let element = info.element;
        let is_inlined = element.is_format_inlined();
        let is_preformatted = tag.is_raw_text() || tag == HtmlTag::pre;

        let prev = self.stack.last().unwrap_or(&DEFAULT_INFO);

        let depth_change = info.depth.cmp(&prev.depth);

        let tabs = match (is_inlined, depth_change) {
            (true, _) | (_, Ordering::Equal) => prev.tabs,
            (_, Ordering::Greater) => prev.tabs + 1,
            (_, Ordering::Less) => prev.tabs,
        };

        let curr = FmtInfo {
            tag,
            tabs,
            depth: info.depth,
            is_preformatted,
        };

        if info.in_inline_sequence {
            self.buf.newline();
        }

        if prev.is_preformatted {
            // push preformatted elements without any modification
            self.buf.open_tag(tag);
            // self.push_start_tag(index, tag, false, false);
            return;
        }

        self.stack.push(curr);
    }
}
