use crate::{
    css::{self, ParseError},
    dom::DomRead,
    html::HtmlDoc,
};

pub fn select(html: &str, css: &str) -> Vec<String> {
    let html = HtmlDoc::parse(html).unwrap();
    html.dom()
        .root()
        .select_css(css)
        .unwrap()
        .map(|el| el.tag_id_class())
        .collect()
}

pub fn try_select(html: &str, css: &str) -> Result<Vec<String>, ParseError> {
    let html = HtmlDoc::parse(html).unwrap();

    let selector = css::parse_css(css)?;
    Ok(html
        .dom()
        .root()
        .select(selector)
        .map(|el| el.tag_id_class())
        .collect())
}
