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
                        classes: vec![ClassSelector("vector-menu-content")],
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
