use std::fs;

use criterion::{Criterion, criterion_group, criterion_main};
use htmlarc_dom::prelude::*;

pub fn iteration(c: &mut Criterion) {
    let doc = fs::read_to_string("src/parser/tests/html/fr.serrer.html").unwrap();

    // let doc = fs::read_to_string("common/html-doc/src/parser/tests/html/en.interest.html").unwrap();
    let html = HtmlDoc::parse(&doc).unwrap().dom();

    c.bench_function(
        "Iteration: forwards safe through all 5589 elements in fr.serrer.html",
        |b| b.iter(|| html.root().forwards().count()),
    );
    c.bench_function(
        "Iteration: forwards through all 5589 elements in fr.serrer.html",
        |b| b.iter(|| html.root().forwards().count()),
    );
}

pub fn iteration2(c: &mut Criterion) {
    let doc = fs::read_to_string("src/parser/tests/html/fr.serrer.html").unwrap();

    // let doc = fs::read_to_string("common/html-doc/src/parser/tests/html/en.interest.html").unwrap();
    let html = HtmlDoc::parse(&doc).unwrap().dom();

    c.bench_function(
        "Iteration: forwards doc through all 5589 elements in fr.serrer.html",
        |b| b.iter(|| html.root().forwards().count()),
    );
}
criterion_group!(benches, iteration, iteration2);
criterion_main!(benches);
