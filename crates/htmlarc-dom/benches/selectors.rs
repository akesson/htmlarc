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

    // Resolve-once id / attribute matching (ADR 0002 §3, PR 3). Each selector is parsed once
    // and cloned per iteration so the per-document resolve pass runs fresh each time, exactly
    // as the engine binds a `'static` selector to a document. `#id` resolves to an attribute
    // entry id (integer scan); attribute names resolve to a `NameSym` (integer prefilter).
    let id = parse_css("#vector-toc").unwrap();
    c.bench_function("select id in fr.serrer.html", |b| {
        b.iter(|| html.root().select(id.clone()).for_each(|_e| {}))
    });

    // `[role="navigation"]` — `role` is case-sensitive, so this is a name prefilter plus a
    // sensitive value compare.
    let attr_exact = parse_css(r#"[role="navigation"]"#).unwrap();
    c.bench_function("select attr exact in fr.serrer.html", |b| {
        b.iter(|| html.root().select(attr_exact.clone()).for_each(|_e| {}))
    });

    // `[typeof="mw:File"]` — case-insensitive default: integer name prefilter, then the
    // lowercased value compare on the (few) name-matching entries.
    let attr_ci = parse_css(r#"[typeof="mw:File"]"#).unwrap();
    c.bench_function("select attr insensitive in fr.serrer.html", |b| {
        b.iter(|| html.root().select(attr_ci.clone()).for_each(|_e| {}))
    });

    // `[data-word]` — an extended (data-*) name resolved to its `NameSym`; presence only.
    let ext_attr = parse_css("[data-word]").unwrap();
    c.bench_function("select ext attr in fr.serrer.html", |b| {
        b.iter(|| html.root().select(ext_attr.clone()).for_each(|_e| {}))
    });

    // Extended (custom) tag selectors (ADR 0002 §4, PR 4). The wiktionary fixture holds no
    // custom elements, so `my-widget` resolves to `Absent` once and the whole walk is a cheap
    // integer non-match — the tag twin of the absent-class prune above.
    let ext_tag_absent = parse_css("my-widget").unwrap();
    c.bench_function("select absent ext tag in fr.serrer.html", |b| {
        b.iter(|| html.root().select(ext_tag_absent.clone()).for_each(|_e| {}))
    });

    // A document of custom elements: `card-item` resolves once to its vocab byte, so matching
    // is a single per-node tag-byte compare. ~2,000 custom elements interleaved with `<div>`s
    // keeps the element count in the same ballpark as the wiktionary fixture.
    let custom_doc: String = (0..2_000)
        .map(|i| format!("<card-item id=\"i{i}\">x</card-item><div>y</div>"))
        .collect();
    let custom_html = HtmlDoc::parse(&format!("<body>{custom_doc}</body>"))
        .unwrap()
        .dom();
    let ext_tag_vocab = parse_css("card-item").unwrap();
    c.bench_function("select ext tag (vocab byte) in custom-elements doc", |b| {
        b.iter(|| {
            custom_html
                .root()
                .select(ext_tag_vocab.clone())
                .for_each(|_e| {})
        })
    });
}

criterion_group!(benches, selectors,);
criterion_main!(benches);
