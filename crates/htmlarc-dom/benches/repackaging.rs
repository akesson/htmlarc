use std::fs;

use criterion::{Criterion, criterion_group, criterion_main};
use htmlarc_dom::prelude::HtmlDoc;

pub fn repackaging(c: &mut Criterion) {
    let html_str = fs::read_to_string("src/parser/tests/html/fr.serrer.html").unwrap();
    let html = HtmlDoc::parse(&html_str).unwrap();

    c.bench_function("repack fr.serrer.html", {
        move |b| {
            b.iter({
                || {
                    html.repackage();
                }
            })
        }
    });
}

criterion_group!(benches, repackaging,);
criterion_main!(benches);
