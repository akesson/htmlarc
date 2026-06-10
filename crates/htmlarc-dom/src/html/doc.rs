use crate::{
    HtmlParseResult,
    parser::{DomBuilder, DomBuilderCursor, parse_into},
    prelude::*,
};
use std::cell::RefCell;
pub struct HtmlDoc {
    pub(crate) dom: DomInner,
}

impl HtmlDoc {
    /// The owned, queryable DOM. Implements [`DomRead`]/[`DomRef`], so it is the
    /// read handle for in-memory documents (mirroring `ArchivedDomInner` for archived
    /// ones); it is also the rkyv-serializable form fed to the archive builder.
    pub fn dom(self) -> DomInner {
        self.dom
    }

    pub fn dom_ref_cell(self) -> DomRefCell {
        DomRefCell {
            dom: RefCell::new(self.dom),
        }
    }

    pub fn parse(input: &str) -> HtmlParseResult<Self> {
        let mut builder = DomBuilderCursor::default();
        parse_into(input, &mut builder)?;
        Ok(builder.dom.into())
    }

    #[cfg(test)]
    pub fn test_parse(input: &str) -> HtmlParseResult<String> {
        let mut dom = crate::parser::TestDom::default();
        parse_into(input, &mut dom)?;
        Ok(dom.to_string())
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
impl From<DomBuilder> for HtmlDoc {
    fn from(value: DomBuilder) -> Self {
        value.build().into()
    }
}
