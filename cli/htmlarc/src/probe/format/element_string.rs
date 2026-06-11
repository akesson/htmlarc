use std::{fmt::Display, hash::Hash};

use smallvec::SmallVec;

use crate::probe::format::*;

/// The ElementString behaves like a string, but the string is only created when
/// used, since it implements Display.
///
/// It also implements equality so that ElementStrings can be compared.
/// This is the point of it: many of these can be cheaply created and compared
/// to each other, and only the ones that are actually used can be turned into strings.
#[derive(Debug, Clone, Eq)]
pub struct ElementString<'dom> {
    /// The style decides how the tag and attributes are formatted.
    pub(super) style: ElementStyle,
    /// The element's tag name (an extended/custom element resolves to its real name, never
    /// the `extended` marker — ADR 0002 §4).
    pub(super) tag_name: &'dom str,
    /// The attributes are the one that matches the selection criteria,
    /// and are thus included in the string.
    pub(super) attrs: SmallVec<[ElementAttribute<'dom>; 4]>,
    pub(crate) with_words: bool,
}

impl Hash for ElementString<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.style.hash(state);
        self.tag_name.hash(state);
        self.attrs.hash(state);
    }
}

impl PartialEq for ElementString<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.style == other.style && self.tag_name == other.tag_name && self.attrs == other.attrs
    }
}

impl Display for ElementString<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.style {
            ElementStyle::HtmlFmt => {
                let (classes, attributes): (Vec<_>, Vec<_>) = self
                    .attrs
                    .iter()
                    .partition(|a| matches!(a, ElementAttribute::Class(_)));

                write!(f, "<{}", self.tag_name)?;

                if !classes.is_empty() {
                    write!(
                        f,
                        " class='{}'",
                        classes
                            .iter()
                            .map(|c| c.format(&self.style))
                            .collect::<Vec<_>>()
                            .join(" ")
                    )?;
                }

                for attr in attributes {
                    write!(f, " {}", attr.format(&self.style))?;
                }
                write!(f, ">")?;
            }
            ElementStyle::CssFmt => {
                write!(f, "{}", self.tag_name)?;

                for attr in self.attrs.iter() {
                    write!(f, "{}", attr.format(&self.style))?;
                }
            }
            ElementStyle::CssTerse => {
                write!(f, "{}", self.tag_name)?;

                for attr in self.attrs.iter() {
                    write!(f, "{}", attr.format(&self.style))?;
                }
            }
        }

        Ok(())
    }
}
