pub mod nodes_tests;

use crate::{iters::ElementIter, prelude::*};
use insta::{assert_snapshot, glob};
use std::fs;

pub fn each_block_element<F>(iter: &mut ElementIter<DomRefCell>, operate: F)
where
    F: Fn(&HtmlElement<DomRefCell>),
{
    for el in iter {
        if el.is_block() {
            operate(&el);
        }
    }
}

// #[test]
// fn roundtrip() {
//     glob!("html/*.html", |path| {
//         let html = fs::read_to_string(path).unwrap();
//         let doc = HtmlDoc::parse(&html).unwrap();

//         assert_snapshot!(doc.to_html(Format::Raw));
//     });
// }

#[test]
fn unwrap_with_space() {
    glob!("html/unwrap/*.html", |path| {
        let html_str = fs::read_to_string(path).unwrap();
        let html = HtmlDoc::parse(&html_str).unwrap().dom_ref_cell();
        html.with_mut(|dom| dom.remove_formatting());
        let mut iter = html.root().forwards();

        each_block_element(&mut iter, |el: &HtmlElement<DomRefCell>| {
            el.unwrap_element();
        });

        assert_snapshot!(html.to_html(HtmlFormat::Raw));
    });
}

#[test]
fn remove_with_space() {
    glob!("html/prune/*.html", |path| {
        let html_str = fs::read_to_string(path).unwrap();
        let html = HtmlDoc::parse(&html_str).unwrap().dom_ref_cell();
        let mut iter = html.root().descendants();
        each_block_element(&mut iter, |el: &HtmlElement<DomRefCell>| {
            el.remove();
        });

        assert_snapshot!(html.to_html(HtmlFormat::Raw));
    });
}

#[test]
fn replace_with_space() {
    let html_str = "<strong>a</strong><em>b</em><div></div><i>c</i>";
    let html = HtmlDoc::parse(html_str).unwrap().dom_ref_cell();
    let el = html.root();
    let el = el.first_child().unwrap();
    let strong = el.index();
    let el = el.next_sibling().unwrap();
    let el = el.next_sibling().unwrap();
    el.replace_with(strong);
    assert_eq!(
        html.to_html(HtmlFormat::Raw),
        "<em>b</em> <strong>a</strong> <i>c</i>",
        "Should add a space before and after a replaced block element surrounded by inline elements"
    );

    let html_str = "<strong>a</strong><em>b</em><div></div><p>c</p>";
    let html = HtmlDoc::parse(html_str).unwrap().dom_ref_cell();
    let el = html.root();
    let el = el.first_child().unwrap();
    let strong = el.index();
    let el = el.next_sibling().unwrap();
    let el = el.next_sibling().unwrap();
    el.replace_with(strong);
    assert_eq!(
        html.to_html(HtmlFormat::Raw),
        "<em>b</em> <strong>a</strong><p>c</p>",
        "Should add a space before a replaced block element preceded by an inline element"
    );

    let html_str = "<strong>a</strong><aside>b</aside><div></div><i>c</i>";
    let html = HtmlDoc::parse(html_str).unwrap().dom_ref_cell();
    let el = html.root();
    let el = el.first_child().unwrap();
    let strong = el.index();
    let el = el.next_sibling().unwrap();
    let el = el.next_sibling().unwrap();
    el.replace_with(strong);
    assert_eq!(
        html.to_html(HtmlFormat::Raw),
        "<aside>b</aside><strong>a</strong> <i>c</i>",
        "Should add a space after a replaced block element followed by an inline element"
    );
}

#[test]
fn unwrap_and_prune() {
    let html_str = "<body><details><p>paragraph</p><summary></summary><summary>texts</summary></details></body>";
    let html = HtmlDoc::parse(html_str).unwrap().dom_ref_cell();
    let el = html.root();
    let el = el.first_child().unwrap();
    let el = el.first_child().unwrap();
    el.unwrap_element();
    assert_eq!(
        html.to_html(HtmlFormat::Raw),
        "<body><p>paragraph</p></body>",
        "Summary elements should be pruned when their Details parent has been unwrapped"
    );

    let html_str = "<body><i>italic</i><section><div><p></p></div></section></body>";
    let html = HtmlDoc::parse(html_str).unwrap().dom_ref_cell();
    let el = html.root();
    let el = el.first_child().unwrap(); // body
    let el = el.first_child().unwrap(); // i
    let el = el.next_sibling().unwrap(); // section
    let el = el.first_child().unwrap(); // div
    let el = el.first_child().unwrap(); // p
    el.unwrap_element();
    assert_eq!(
        html.to_html(HtmlFormat::Raw),
        "<body><i>italic</i></body>",
        "After unwrapping an empty element, its parent should be pruned as well"
    );

    let html_str =
        "<body><i>italic</i><section><div><span>   </span><p></p></div><b> </b></section></body>";
    let html = HtmlDoc::parse(html_str).unwrap().dom_ref_cell();
    let el = html.root();
    let el = el.first_child().unwrap(); // body
    let el = el.first_child().unwrap(); // i
    let el = el.next_sibling().unwrap(); // section
    let el = el.first_child().unwrap(); // div
    let el = el.first_child().unwrap(); // span
    let el = el.next_sibling().unwrap(); // p
    el.unwrap_element();
    assert_eq!(
        html.to_html(HtmlFormat::Raw),
        "<body><i>italic</i></body>",
        "After unwrapping an empty element, its parent should be pruned as well if they only contain white spaces"
    );

    let html_str = "<body><i>italic</i><section><div><p></p></div></section><b>bold</b></body>";
    let html = HtmlDoc::parse(html_str).unwrap().dom_ref_cell();
    let el = html.root();
    let el = el.first_child().unwrap(); // body
    let el = el.first_child().unwrap(); // i
    let el = el.next_sibling().unwrap(); // section
    let el = el.first_child().unwrap(); // div
    let el = el.first_child().unwrap(); // p
    el.unwrap_element();
    assert_eq!(
        html.to_html(HtmlFormat::Raw),
        "<body><i>italic</i> <b>bold</b></body>",
        "After unwraping and pruning an empty element, replace it with a space if it's a block element between inline elements"
    );
}

#[test]
fn replace_and_prune() {
    let html_str =
        "<body><section><div><p>paragraph</p></div></section><article><img></article></body>";
    let html = HtmlDoc::parse(html_str).unwrap().dom_ref_cell();
    let el = html.root();
    let el = el.first_child().unwrap(); // body
    let el = el.first_child().unwrap(); // section
    let el = el.first_child().unwrap(); // div
    let el = el.first_child().unwrap(); // p
    let p = el.index();
    let el = el.parent().unwrap(); // div
    let el = el.parent().unwrap(); // section
    let el = el.next_sibling().unwrap(); // article
    let el = el.last_child_all().unwrap(); // img
    el.replace_with(p);

    assert_eq!(
        html.to_html(HtmlFormat::Raw),
        "<body><article><p>paragraph</p></article></body>",
        "After replacing an element, the replacement's original parent and ancestors should be pruned if empty"
    );

    let html_str = "<body><section><div><p>paragraph</p><span>  </span></div><i> </i></section><article><img></article></body>";
    let html = HtmlDoc::parse(html_str).unwrap().dom_ref_cell();
    let el = html.root();
    let el = el.first_child().unwrap(); // body
    let el = el.first_child().unwrap(); // section
    let el = el.first_child().unwrap(); // div
    let el = el.first_child().unwrap(); // p
    let p = el.index();
    let el = el.parent().unwrap(); // div
    let el = el.parent().unwrap(); // section
    let el = el.next_sibling().unwrap(); // article
    let el = el.first_child().unwrap(); // img
    el.replace_with(p);

    assert_eq!(
        html.to_html(HtmlFormat::Raw),
        "<body><article><p>paragraph</p></article></body>",
        "After replacing an element, the replacement's original parent and ancestors should be pruned if they only contain white spaces"
    );

    let html_str = "<body><i>italic</i><section><div><p>paragraph</p></div></section><b>bold</b><article><img></article></body>";
    let html = HtmlDoc::parse(html_str).unwrap().dom_ref_cell();
    let el = html.root();
    let el = el.first_child().unwrap(); // body
    let el = el.first_child().unwrap(); // i
    let el = el.next_sibling().unwrap(); // section
    let el = el.first_child().unwrap(); // div
    let el = el.first_child().unwrap(); // p
    let p = el.index();
    let el = el.parent().unwrap(); // div
    let el = el.parent().unwrap(); // section
    let el = el.next_sibling().unwrap(); // b
    let el = el.next_sibling().unwrap(); // article
    let el = el.first_child().unwrap(); // img
    el.replace_with(p);

    assert_eq!(
        html.to_html(HtmlFormat::Raw),
        "<body><i>italic</i> <b>bold</b><article><p>paragraph</p></article></body>",
        "After replacing an element, the replacement's original parent and ancestors should be pruned and replaced with a space if the outer parent is a block element in between inline elements"
    );
}

#[test]
fn remove_children() {
    let html_str = "<body><i>italic</i><section><div><p>paragraph</p></div></section></body>";
    let html = HtmlDoc::parse(html_str).unwrap().dom_ref_cell();

    let el = html.root();
    let el = el.first_child().assert(HtmlTag::body);
    let el = el.first_child().assert(HtmlTag::i);
    let el = el.next_sibling().assert(HtmlTag::section);
    el.remove_children();

    assert_eq!(
        html.to_html(HtmlFormat::Raw),
        "<body><i>italic</i><section></section></body>",
        "No children left"
    );
}

#[test]
fn remove_and_prune() {
    let html_str = "<body><i>italic</i><section><div><p></p></div></section></body>";
    let html = HtmlDoc::parse(html_str).unwrap().dom_ref_cell();
    let el = html.root();
    let el = el.first_child().unwrap(); // body
    let el = el.first_child().unwrap(); // space
    let el = el.next_sibling().unwrap(); // section
    let el = el.first_child().unwrap(); // div
    let el = el.first_child().unwrap(); // p
    let el = el.remove().unwrap();

    assert_eq!(
        html.to_html(HtmlFormat::Raw),
        "<body><i>italic</i></body>",
        "After removing an element, its parent should be pruned if they are empty"
    );
    assert_eq!(
        el.tag(),
        HtmlTag::i,
        "The element cursor should be repositioned"
    );

    let html_str =
        "<body><i>italic</i><section><b> </b><div><p></p><span> </span></div></section></body>";
    let html = HtmlDoc::parse(html_str).unwrap().dom_ref_cell();
    let el = html.root();
    let el = el.first_child().unwrap(); // body
    let el = el.first_child().unwrap(); // i
    let el = el.next_sibling().unwrap(); // section
    let el = el.last_child_all().unwrap(); // div
    let el = el.first_child().unwrap(); // p
    el.remove();

    assert_eq!(
        html.to_html(HtmlFormat::Raw),
        "<body><i>italic</i></body>",
        "After removing an element, its parent should be pruned if the outer parent contains only white spaces"
    );

    let html_str = "<body><i>italic</i><section><div><p></p></div></section><b>bold</b></body>";
    let html = HtmlDoc::parse(html_str).unwrap().dom_ref_cell();
    let el = html.root();
    let el = el.first_child().unwrap(); // body
    let el = el.first_child().unwrap(); // space
    let el = el.next_sibling().unwrap(); // section
    let el = el.first_child().unwrap(); // div
    let el = el.first_child().unwrap(); // p
    el.remove();

    assert_eq!(
        html.to_html(HtmlFormat::Raw),
        "<body><i>italic</i> <b>bold</b></body>",
        "After removing an element, its parent should be pruned and replaced with a space if the outer parent is a block element in between inline elements"
    );
}
