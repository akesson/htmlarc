mod attributes;
mod builder;
mod chars;
mod doc;
mod dom;
mod lines;
mod tags;
#[cfg(test)]
mod testdom;
#[cfg(test)]
mod tests;

use dom::DomStack;

pub(crate) use self::chars::Chars;
pub(crate) use builder::{DomBuilder, DomBuilderCursor};
pub(crate) use doc::parse_doc;

#[cfg(test)]
pub use testdom::TestDom;

#[cfg(test)]
use crate::{ParseResult, SelectorError};

#[cfg(test)]
fn with_chars<R, F: Fn(&mut Chars) -> R>(html: &str, f: F) -> R {
    f(&mut Chars::new(html))
}

#[cfg(test)]
fn with_chars_check_last<R, F: FnMut(&mut Chars) -> ParseResult<R>>(
    html: &str,
    mut f: F,
    c: char,
) -> ParseResult<R> {
    let mut chars = Chars::new(html);
    let ret = f(&mut chars)?;
    if chars.current() != c {
        Err(SelectorError::new(format!(
            "Expected {c} but was: {}",
            chars.current()
        )))
    } else {
        Ok(ret)
    }
}

#[cfg(test)]
fn with_chars_and_dom<'a, F: Fn(&mut testdom::TestDom, &mut Chars<'a>) -> ParseResult<()>>(
    s: &'a str,
    f: F,
    c: char,
) -> String {
    let mut dom = testdom::TestDom::new();
    let mut chars = Chars::new(s);
    if let Err(e) = f(&mut dom, &mut chars) {
        return e.to_string();
    }
    let current = chars.current();
    if c != current {
        return format!("Expected current to be '{c}', not '{current}'");
    }
    dom.to_string().trim_matches('\n').to_string()
}
