use crate::{
    dom::DomView,
    fmt::{
        fmt_buf::FmtBuf,
        iter::{ElementInfo, Inliner, TagStage},
    },
    html::HtmlTag,
};

use super::common::CommonFormatting;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Inline {
    Start,
    Inside,
    /// The previous element was inline, but this one is not
    Ended,
    None,
}

impl Inline {
    fn next(&mut self, inlined: bool) {
        *self = if inlined {
            match self {
                Self::Start | Self::Inside => Self::Inside,
                Self::Ended | Self::None => Self::Start,
            }
        } else {
            match self {
                Self::Start | Self::Inside => Self::Ended,
                Self::Ended | Self::None => Self::None,
            }
        }
    }
}

pub struct PrettyFormat<'dom> {
    dom: DomView<'dom>,
    buf: FmtBuf,
    inline: Inline,
    prev_index: u16,
}
impl<'dom> CommonFormatting<'dom> for PrettyFormat<'dom> {
    fn dom_and_buf(&mut self) -> (DomView<'dom>, &mut FmtBuf) {
        (self.dom, &mut self.buf)
    }
}

impl<'dom> PrettyFormat<'dom> {
    pub fn new(dom: DomView<'dom>) -> Self {
        Self {
            dom,
            buf: Default::default(),
            inline: Inline::None,
            prev_index: 0,
        }
    }

    pub fn html(mut self, index: u16) -> String {
        for info in Inliner::new(self.dom, index) {
            let tag = info.tag();
            let index = info.index();
            self.inline.next(info.in_inline_sequence);

            match info.stage {
                TagStage::Open => match tag {
                    HtmlTag::DOCTYPE => self.add_doctype(index),
                    HtmlTag::sys_comment => self.add_comment(self.dom.string_at(index)),
                    HtmlTag::sys_text => {
                        if !self.push_trimmed_text(index, self.inline)
                            && self.inline == Inline::Start
                        {
                            self.inline = Inline::None;
                        }
                    }
                    _ => self.add_start_tag(info, tag),
                },
                TagStage::Close => self.add_close_tag(info, tag),
            }
            self.prev_index = index;
        }
        self.buf.inner()
    }

    fn add_comment(&mut self, comment: &str) {
        self.buf.push_str("\n<!--");
        self.buf.push_str(comment);
        self.buf.push_str("-->");
    }

    fn add_start_tag(&mut self, info: ElementInfo, tag: HtmlTag) {
        let index = info.index();
        match self.inline {
            Inline::Start => {
                self.buf.newline();
            }
            Inline::Ended | Inline::None => {
                self.buf.newline_and_indent(info.depth);
            }
            Inline::Inside => {}
        }

        self.buf.push('<');
        self.buf.push_str(tag.into());
        self.push_attributes(index);
        if tag.auto_close() {
            self.buf.push(' ');
            self.buf.push('/');
        }
        self.buf.push('>');
    }

    fn add_close_tag(&mut self, info: ElementInfo, tag: HtmlTag) {
        if tag.no_close() {
            return;
        }
        let no_children = self.prev_index == info.index();

        if matches!(self.inline, Inline::Ended | Inline::None) && !no_children {
            self.buf.newline_and_indent(info.depth);
        }
        self.buf.push_str("</");
        self.buf.push_str(tag.into());
        self.buf.push('>');
    }
}
