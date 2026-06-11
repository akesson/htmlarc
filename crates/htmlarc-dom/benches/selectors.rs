use std::fs;

use criterion::{Criterion, criterion_group, criterion_main};
use htmlarc_dom::css::*;
use htmlarc_dom::prelude::*;

pub fn selectors(c: &mut Criterion) {
    let doc = fs::read_to_string("src/parser/tests/html/fr.serrer.html").unwrap();

    // let doc = fs::read_to_string("common/html-doc/src/parser/tests/html/en.interest.html").unwrap();
    let html = HtmlDoc::parse(&doc).unwrap().dom();

    // html.root_element().select(selector);
    c.bench_function("select divs in fr.serrer.html", |b| {
        b.iter(|| {
            let selector = SelectorList {
                selectors: vec![ComplexSelector {
                    first: CompoundSelector {
                        element: Some(HtmlTag::div),
                        ..Default::default()
                    },
                    selectors: Vec::new(),
                }],
            };
            html.root().select(selector).for_each(|_e| {})
        })
    });

    c.bench_function("select class in fr.serrer.html", |b| {
        b.iter(|| {
            let selector = SelectorList {
                selectors: vec![ComplexSelector {
                    first: CompoundSelector {
                        classes: vec![ClassSelector::new("vector-menu-content")],
                        ..Default::default()
                    },
                    selectors: Vec::new(),
                }],
            };

            html.root().select(selector).for_each(|_e| {})
        })
    });

    // A class absent from the document: the resolve pass binds it to `Absent` once, so the
    // compound can never match and the whole walk is a per-node integer check against a
    // selector that is known not to match. The pre-resolve string path could not prune this.
    c.bench_function("select absent class in fr.serrer.html", |b| {
        b.iter(|| {
            let selector = SelectorList {
                selectors: vec![ComplexSelector {
                    first: CompoundSelector {
                        classes: vec![ClassSelector::new("this-class-does-not-exist")],
                        ..Default::default()
                    },
                    selectors: Vec::new(),
                }],
            };

            html.root().select(selector).for_each(|_e| {})
        })
    });

    // A multi-class compound: every candidate node is checked against two resolved Syms,
    // exercising the per-node integer-compare loop more than once per element.
    c.bench_function("select multi-class in fr.serrer.html", |b| {
        b.iter(|| {
            let selector = SelectorList {
                selectors: vec![ComplexSelector {
                    first: CompoundSelector {
                        classes: vec![
                            ClassSelector::new("vector-menu-content"),
                            ClassSelector::new("vector-menu"),
                        ],
                        ..Default::default()
                    },
                    selectors: Vec::new(),
                }],
            };

            html.root().select(selector).for_each(|_e| {})
        })
    });
}

criterion_group!(benches, selectors,);
criterion_main!(benches);
