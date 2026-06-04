use std::fs;

use criterion::{Criterion, criterion_group, criterion_main};
use htmlarc_dom::prelude::HtmlDoc;

pub fn parsing(c: &mut Criterion) {
    let doc = fs::read_to_string("src/parser/tests/html/fr.serrer.html").unwrap();

    // let doc = fs::read_to_string("common/html-doc/src/parser/tests/html/en.interest.html").unwrap();

    c.bench_function("parse fr.serrer.html", |b| {
        b.iter(|| HtmlDoc::parse(&doc).unwrap())
    });
}

criterion_group!(benches, parsing,);
criterion_main!(benches);
