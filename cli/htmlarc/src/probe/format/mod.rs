mod attribute_selector;
mod element_attr;
mod element_string;
mod element_style;

use std::fmt::Display;

pub use element_attr::*;
pub use element_string::*;
pub use element_style::*;
pub(super) use htmlarc_dom::{css::*, prelude::*};
use smallvec::SmallVec;

pub(super) use crate::*;

#[derive(Debug, Clone)]
pub struct ElementFormat<'s> {
    style: ElementStyle,
    selectors: Vec<attribute_selector::AttributeSelector<'s>>,
    with_words: bool,
}

impl<'s> TryFrom<&'s str> for ElementFormat<'s> {
    type Error = String;

    fn try_from(s: &'s str) -> Result<Self, Self::Error> {
        let (style, str) = if let Some(str) = s.strip_prefix("HtmlFmt") {
            (ElementStyle::HtmlFmt, str)
        } else if let Some(str) = s.strip_prefix("CssFmt") {
            (ElementStyle::CssFmt, str)
        } else if let Some(str) = s.strip_prefix("CssTerse") {
            (ElementStyle::CssTerse, str)
        } else {
            return Err("no prefix".to_string());
        };
        debug!("style: {:?}", style);
        debug!("str: {:?}", str);

        let mut chars = CssChars::new(str);
        let mut selectors = Vec::new();
        let mut with_words = false;

        while let Some((index, char)) = chars.current() {
            match char {
                '[' => {
                    if let Some(attribute) =
                        attribute_selector::AttributeSelector::from_chars(&mut chars)
                            .map_err(|e| e.to_string())?
                    {
                        debug!("Added attribute : {}", attribute);
                        selectors.push(attribute);
                    } else {
                        return Err("no content".to_string());
                    }
                }
                '@' => {
                    const WORD_PARAM: &str = "words";

                    chars.next();
                    let pattern = TextPattern::default()
                        .allow_alphabetic()
                        .start_with(CssChar::Alphabetic);

                    if let Some(text) = pattern.validate(&mut chars).map_err(|e| e.to_string())? {
                        if text == WORD_PARAM {
                            with_words = true;
                        } else {
                            return Err(format!("unexpected parameter '{}'", text));
                        }
                    } else {
                        return Err("Expected words parameter".to_string());
                    }
                }
                _ => {
                    return Err(format!(
                        "unexpected character '{}' at index {}",
                        char, index
                    ));
                }
            }
        }

        Ok(Self {
            style,
            selectors,
            with_words,
        })
    }
}

impl Display for ElementFormat<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let style = match self.style {
            ElementStyle::HtmlFmt => "HtmlFmt",
            ElementStyle::CssFmt => "CssFmt",
            ElementStyle::CssTerse => "CssTerse",
        };
        let selectors = self
            .selectors
            .iter()
            .map(|selector| selector.to_string())
            .collect::<Vec<_>>()
            .join("");
        write!(f, "{}{}", style, selectors)
    }
}

impl<'dom> ElementFormat<'dom> {
    /// Formats an element
    pub fn format<Dom: DomRef>(&self, element: &HtmlElement<'dom, Dom>) -> ElementString<'dom> {
        let mut attrs: SmallVec<[ElementAttribute<'dom>; 4]> = SmallVec::new();

        for selector in &self.selectors {
            match selector.name {
                AttributeName::Text => {
                    let el_text = element.text_content();
                    if let Some((operator, pattern)) = selector.value {
                        if !el_text.is_empty() {
                            debug!("text: {:?}", el_text);
                            if operator.matches(pattern, &el_text) {
                                attrs.push(ElementAttribute::Text(el_text));
                            }
                        }
                    } else if !el_text.is_empty() {
                        attrs.push(ElementAttribute::Text(el_text));
                    }
                }
                AttributeName::Html(html_attr) => {
                    if html_attr == HtmlAttr::class {
                        if let Some((operator, pattern)) = selector.value {
                            for class in element.classes() {
                                if operator.matches(pattern, class) {
                                    let class = Class::from(class);
                                    attrs.push(ElementAttribute::Class(class));
                                }
                            }
                        } else {
                            for class in element.classes() {
                                let class = Class::from(class);
                                attrs.push(ElementAttribute::Class(class));
                            }
                        }
                    } else if let Some((operator, pattern)) = selector.value {
                        for attr in element.attributes() {
                            if attr.tag == html_attr && operator.matches(pattern, attr.val) {
                                attrs.push(ElementAttribute::Attribute(attr));
                            }
                        }
                    } else {
                        for attr in element.attributes() {
                            if attr.tag == html_attr {
                                attrs.push(ElementAttribute::Attribute(attr));
                            }
                        }
                    }
                }
                AttributeName::Data(data) => {
                    if let Some((operator, pattern)) = selector.value {
                        for attr in element.data_attributes() {
                            if attr.tag == data && operator.matches(pattern, attr.val) {
                                attrs.push(ElementAttribute::DataAttribute(attr));
                            }
                        }
                    } else {
                        for attr in element.data_attributes() {
                            if attr.tag == data {
                                attrs.push(ElementAttribute::DataAttribute(attr));
                            }
                        }
                    }
                }
            }
        }

        ElementString {
            style: self.style,
            tag: element.tag(),
            attrs,
            with_words: self.with_words,
        }
    }
}

#[test]
fn parse_element_format() {
    let str = "HtmlFmt[class*=mw][class$=bla][data-bar][text*=bla]";
    let fmt = ElementFormat::try_from(str).unwrap();
    assert_eq!(str, fmt.to_string(), "HtmlFmt parsing");

    let str = "CssFmt[id][class*=mw][class$=bla][data-bar][text*=bla]";
    let fmt = ElementFormat::try_from(str).unwrap();
    assert_eq!(str, fmt.to_string(), "CssFmt parsing");

    let str = "CssTerse[id][class*=mw][class$=bla][data-bar][text*=bla]";
    let fmt = ElementFormat::try_from(str).unwrap();
    assert_eq!(str, fmt.to_string(), "CssTerse parsing");
}

#[cfg(test)]
const HTML: &str =
    r#"<div id="div" class="blue yellow pink" data-foo="bar" title="title">paragraph</div>"#;

#[test]
fn test_words_parameter() {
    let fmt = ElementFormat::try_from("HtmlFmt[class*=mw]@words").unwrap();
    assert!(fmt.with_words);

    let fmt = ElementFormat::try_from("HtmlFmt[class*=mw]@words").unwrap();
    assert!(fmt.with_words);

    let fmt = ElementFormat::try_from("HtmlFmt[class*=mw]@words").unwrap();
    assert!(fmt.with_words,);
}

#[test]
fn test_html_fmt() {
    let dom = HtmlDoc::parse(HTML).unwrap().dom();
    let root = dom.root();
    let el = root.forwards().next().unwrap();

    let fmt = ElementFormat::try_from("HtmlFmt[class]").unwrap();
    let out = fmt.format(&el).to_string();
    assert_eq!(out, "<div class='blue yellow pink'>");

    let fmt = ElementFormat::try_from("HtmlFmt[data-foo]").unwrap();
    let out = fmt.format(&el).to_string();
    assert_eq!(out, "<div data-foo='bar'>");

    let fmt = ElementFormat::try_from("HtmlFmt[id]").unwrap();
    let out = fmt.format(&el).to_string();
    assert_eq!(out, "<div id='div'>");

    let fmt = ElementFormat::try_from("HtmlFmt[text]").unwrap();
    let out = fmt.format(&el).to_string();
    assert_eq!(out, "<div text='paragraph'>");

    let fmt = ElementFormat::try_from("HtmlFmt[id^=d][class*=l][data-foo$=r][text*=rag]").unwrap();
    let out = fmt.format(&el).to_string();
    assert_eq!(
        out,
        "<div class='blue yellow' id='div' data-foo='bar' text='paragraph'>"
    );

    let fmt = ElementFormat::try_from("HtmlFmt[id^=x][class*=x][data-foo*=x][text*=x]").unwrap();
    let out = fmt.format(&el).to_string();
    assert_eq!(out, "<div>");
}

#[test]
fn test_css_fmt() {
    let dom = HtmlDoc::parse(HTML).unwrap().dom();
    let root = dom.root();
    let el = root.forwards().next().unwrap();

    let fmt = ElementFormat::try_from("CssFmt[class]").unwrap();
    let out = fmt.format(&el).to_string();
    assert_eq!(out, "div.blue.yellow.pink");

    let fmt = ElementFormat::try_from("CssFmt[data-foo]").unwrap();
    let out = fmt.format(&el).to_string();
    assert_eq!(out, "div[data-foo='bar']");

    let fmt = ElementFormat::try_from("CssFmt[id]").unwrap();
    let out = fmt.format(&el).to_string();
    assert_eq!(out, "div#div");

    let fmt = ElementFormat::try_from("CssFmt[text]").unwrap();
    let out = fmt.format(&el).to_string();
    assert_eq!(out, "div[text='paragraph']");

    let fmt = ElementFormat::try_from("CssFmt[id^=d][class*=l][data-foo$=r][text*=rag]").unwrap();
    let out = fmt.format(&el).to_string();
    assert_eq!(out, "div#div.blue.yellow[data-foo='bar'][text='paragraph']");

    let fmt = ElementFormat::try_from("CssFmt[id^=x][class*=x][data-foo*=x][text*=x]").unwrap();
    let out = fmt.format(&el).to_string();
    assert_eq!(out, "div");
}

#[test]
fn test_css_terse() {
    let dom = HtmlDoc::parse(HTML).unwrap().dom();
    let root = dom.root();
    let el = root.forwards().next().unwrap();

    let fmt = ElementFormat::try_from("CssTerse[class]").unwrap();
    let out = fmt.format(&el).to_string();
    assert_eq!(out, "div.blue.yellow.pink");

    let fmt = ElementFormat::try_from("CssTerse[data-foo]").unwrap();
    let out = fmt.format(&el).to_string();
    assert_eq!(out, "div['bar']");

    let fmt = ElementFormat::try_from("CssTerse[id]").unwrap();
    let out = fmt.format(&el).to_string();
    assert_eq!(out, "div#div");

    let fmt = ElementFormat::try_from("CssTerse[text]").unwrap();
    let out = fmt.format(&el).to_string();
    assert_eq!(out, "div['paragraph']");

    let fmt = ElementFormat::try_from("CssTerse[id^=d][class*=l][data-foo$=r][text*=rag]").unwrap();
    let out = fmt.format(&el).to_string();
    assert_eq!(out, "div#div.blue.yellow['bar']['paragraph']");

    let fmt = ElementFormat::try_from("CssTerse[id^=x][class*=x][data-foo*=x][text*=x]").unwrap();
    let out = fmt.format(&el).to_string();
    assert_eq!(out, "div");
}
