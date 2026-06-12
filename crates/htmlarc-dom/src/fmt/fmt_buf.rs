use crate::entities;
use crate::html::foreign;
use crate::stores::{AttrName, Attribute, Class};

#[derive(Default)]
pub struct FmtBuf(String);

impl FmtBuf {
    pub fn push_str(&mut self, string: &str) {
        self.0.push_str(string);
    }

    pub fn push(&mut self, char: char) {
        self.0.push(char);
    }

    pub fn newline_and_indent(&mut self, count: u16) {
        self.push('\n');
        self.push_str(&"\t".repeat(count as usize));
    }

    pub fn newline(&mut self) {
        self.push('\n');
    }

    pub fn add_comment(&mut self, comment: &str) {
        self.push_str("<!--");
        self.push_str(comment);
        self.push_str("-->");
    }
    pub fn inner(self) -> String {
        self.0
    }

    pub fn add_classes<'a>(&mut self, list: impl Iterator<Item = Class<'a>>) {
        self.push(' ');
        self.push_str("class=\"");
        for (i, class) in list.enumerate() {
            if i != 0 {
                self.push(' ');
            }
            self.push_str(class.0);
        }
        self.push('"');
    }

    pub fn add_attrs<'a>(&mut self, list: impl Iterator<Item = Attribute<'a>>) {
        for entry in list {
            self.push(' ');
            let Attribute { name, val } = entry;
            // Standard names render as-is; an extended name may be a known SVG/MathML
            // attribute whose canonical case is restored here (ADR 0002 §5).
            match name {
                AttrName::Std(attr) => self.push_str(<&'static str>::from(attr)),
                AttrName::Ext(ext) => self.push_str(foreign::adjust_attr_name(ext)),
            }
            // An empty value renders as a bare attribute (`disabled`, `data-x`).
            if !val.is_empty() {
                self.push_str("=\"");
                self.push_str(&entities::encode_attr(val));
                self.push('"');
            }
        }
    }
}
