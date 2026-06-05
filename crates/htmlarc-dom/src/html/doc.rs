use crate::{
    HtmlParseResult,
    parser::{Chars, DomBuilder, DomBuilderCursor, parse_doc},
    prelude::*,
};
use rkyv::{rancor::Error, util::AlignedVec};
use std::cell::RefCell;
pub struct HtmlDoc {
    pub(crate) dom: DomInner,
}

impl HtmlDoc {
    pub fn dom(self) -> DomOwn {
        DomOwn { dom: self.dom }
    }

    pub fn dom_ref_cell(self) -> DomRefCell {
        DomRefCell {
            dom: RefCell::new(self.dom),
        }
    }

    pub fn inner(self) -> DomInner {
        self.dom
    }

    pub fn parse(input: &str) -> HtmlParseResult<Self> {
        let mut builder = DomBuilderCursor::default();
        let mut chars = Chars::new(input);
        parse_doc(&mut builder, &mut chars)?;
        Ok(builder.dom.into())
    }

    #[cfg(test)]
    pub fn test_parse(input: &str) -> HtmlParseResult<String> {
        let mut dom = crate::parser::TestDom::default();
        let mut chars = Chars::new(input);
        parse_doc(&mut dom, &mut chars)?;
        Ok(dom.to_string())
    }
    pub fn to_bytes(&self) -> AlignedVec {
        rkyv::to_bytes::<Error>(&self.dom).unwrap()
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        unsafe {
            rkyv::from_bytes_unchecked::<DomInner, Error>(bytes)
                .unwrap()
                .into()
        }
    }

    pub fn write_to(&self, path: &std::path::Path) {
        let data = self.to_bytes();
        std::fs::write(path, data).unwrap();
    }

    pub fn to_html(&self, fmt: HtmlFormat) -> String {
        self.dom.to_html(fmt)
    }

    pub fn repackage(&self) -> Self {
        self.dom.rebuild().into()
    }
}

impl From<DomInner> for HtmlDoc {
    fn from(dom: DomInner) -> Self {
        Self { dom }
    }
}
impl From<DomBuilder<'_>> for HtmlDoc {
    fn from(value: DomBuilder) -> Self {
        value.build().into()
    }
}
