use std::cell::Cell;

use crate::{
    dom::{DomRead, NodeIndex},
    html::HtmlElement,
};

use super::DomIterator;

enum Relatives {
    Ancestors,
    Children,
    PrevSiblings,
    NextSiblings,
}

pub struct RelativeIter<'dom, Dom> {
    dom: &'dom Dom,
    relatives: Relatives,
    cursor: Cell<NodeIndex>,
    first: Cell<bool>,
    include_text: bool,
    include_comment: bool,
}

impl<'dom, Dom: DomRead> RelativeIter<'dom, Dom> {
    pub fn ancestors(el: &HtmlElement<'dom, Dom>) -> Self {
        Self::new(el, Relatives::Ancestors)
    }
    pub fn children(el: &HtmlElement<'dom, Dom>) -> Self {
        Self::new(el, Relatives::Children)
    }
    pub fn prev_siblings(el: &HtmlElement<'dom, Dom>) -> Self {
        Self::new(el, Relatives::PrevSiblings)
    }
    pub fn next_siblings(el: &HtmlElement<'dom, Dom>) -> Self {
        Self::new(el, Relatives::NextSiblings)
    }

    fn new(el: &HtmlElement<'dom, Dom>, relatives: Relatives) -> Self {
        Self {
            dom: el.dom,
            relatives,
            cursor: el.index().into(),
            first: true.into(),
            include_text: false,
            include_comment: false,
        }
    }

    #[cfg(test)]
    fn string(self) -> String {
        self.map(|el| format!("{}", el.tag()))
            .collect::<Vec<String>>()
            .join(", ")
    }
}

impl<'dom, Dom: DomRead> Iterator for RelativeIter<'dom, Dom> {
    type Item = HtmlElement<'dom, Dom>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_element()
    }
}

impl<'dom, Dom: DomRead> DomIterator<'dom, Dom> for RelativeIter<'dom, Dom> {
    fn dom(&self) -> &'dom Dom {
        self.dom
    }

    fn next_index(&self) -> Option<NodeIndex> {
        let current = HtmlElement::new(self.dom, self.cursor.get());

        if let Some(next) = match self.relatives {
            Relatives::Ancestors => current.parent().ok(),
            Relatives::Children => {
                if self.first.get() {
                    self.first.set(false);
                    current.first_child_all()
                } else {
                    current.next_sibling_all()
                }
            }
            Relatives::PrevSiblings => current.prev_sibling_all(),
            Relatives::NextSiblings => current.next_sibling_all(),
        } {
            self.cursor.set(next.index());

            Some(next.index())
        } else {
            None
        }
    }

    fn set_include_comment(mut self) -> Self {
        self.include_comment = true;
        self
    }

    fn include_comment(&self) -> bool {
        self.include_comment
    }

    fn set_include_text(mut self) -> Self {
        self.include_text = true;
        self
    }

    fn include_text(&self) -> bool {
        self.include_text
    }
}

#[cfg(test)]
const HTML: &str = r#"
<header>
    <h1></h1>
</header>
<main>
    <section>
        <p></p><!-- comment -->
        <article></article>
        <div>
            <span></span>
            <h2></h2>
            <!-- comment -->
            <p></p>
        </div>
        <a></a><!-- comment -->
        <h4></h4>
    </section>
</main>
"#;

#[cfg(test)]
fn div<'a>(root: &'a HtmlElement<impl DomRead>) -> HtmlElement<'a, impl DomRead> {
    let header = root.first_child().unwrap();
    let main = header.next_sibling().unwrap();
    let section = main.first_child().unwrap();
    let p = section.first_child().unwrap();
    let article = p.next_sibling().unwrap();
    article.next_sibling().unwrap()
}

#[test]
fn test_relative_ancestors() {
    let dom = crate::html::HtmlDoc::parse(HTML).unwrap().dom();
    let root = dom.root();
    let div = div(&root);

    let ancestors = RelativeIter::new(&div, Relatives::Ancestors).string();

    assert_eq!(ancestors, "section, main, sys_root");
}

#[test]
fn test_relative_children() {
    let dom = crate::html::HtmlDoc::parse(HTML).unwrap().dom();
    let root = dom.root();
    let div = div(&root);

    let children = RelativeIter::new(&div, Relatives::Children).string();

    assert_eq!(children, "span, h2, p");
}

#[test]
fn test_relative_next_siblings() {
    let dom = crate::html::HtmlDoc::parse(HTML).unwrap().dom();
    let root = dom.root();
    let div = div(&root);

    let next_siblings = RelativeIter::new(&div, Relatives::NextSiblings).string();

    assert_eq!(next_siblings, "a, h4");
}

#[test]
fn test_relative_prev_siblings() {
    let dom = crate::html::HtmlDoc::parse(HTML).unwrap().dom();
    let root = dom.root();
    let div = div(&root);

    let prev_siblings = RelativeIter::new(&div, Relatives::PrevSiblings).string();

    assert_eq!(prev_siblings, "article, p");
}

#[test]
fn test_relative_children_with_text_and_comments() {
    let dom = crate::html::HtmlDoc::parse(HTML).unwrap().dom();
    let root = dom.root();
    let div = div(&root);

    let children = RelativeIter::new(&div, Relatives::Children)
        .set_include_comment()
        .string();

    assert_eq!(children, "span, h2, comment, p");

    let children = RelativeIter::new(&div, Relatives::Children)
        .set_include_text()
        .string();

    assert_eq!(children, "text, span, text, h2, text, text, p, text");

    let children = RelativeIter::new(&div, Relatives::Children)
        .set_include_comment()
        .set_include_text()
        .string();

    assert_eq!(
        children,
        "text, span, text, h2, text, comment, text, p, text"
    );
}

#[test]
fn test_relative_next_siblings_with_text_and_comments() {
    let dom = crate::html::HtmlDoc::parse(HTML).unwrap().dom();
    let root = dom.root();
    let div = div(&root);

    let next_siblings = RelativeIter::new(&div, Relatives::NextSiblings)
        .set_include_comment()
        .string();

    assert_eq!(next_siblings, "a, comment, h4");

    let next_siblings = RelativeIter::new(&div, Relatives::NextSiblings)
        .set_include_text()
        .string();

    assert_eq!(next_siblings, "text, a, text, h4, text");

    let next_siblings = RelativeIter::new(&div, Relatives::NextSiblings)
        .set_include_comment()
        .set_include_text()
        .string();

    assert_eq!(next_siblings, "text, a, comment, text, h4, text");
}

#[test]
fn test_relative_prev_siblings_with_text_and_comments() {
    let dom = crate::html::HtmlDoc::parse(HTML).unwrap().dom();
    let root = dom.root();
    let div = div(&root);

    let prev_siblings = RelativeIter::new(&div, Relatives::PrevSiblings)
        .set_include_comment()
        .string();

    assert_eq!(prev_siblings, "article, comment, p");

    let prev_siblings = RelativeIter::new(&div, Relatives::PrevSiblings)
        .set_include_text()
        .string();

    assert_eq!(prev_siblings, "text, article, text, p, text");

    let prev_siblings = RelativeIter::new(&div, Relatives::PrevSiblings)
        .set_include_comment()
        .set_include_text()
        .string();

    assert_eq!(prev_siblings, "text, article, text, comment, p, text");
}
