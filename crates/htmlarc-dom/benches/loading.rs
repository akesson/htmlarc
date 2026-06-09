use criterion::{Criterion, criterion_group, criterion_main};
use htmlarc_dom::prelude::*;
use rkyv::rancor::Error;
use std::fs;

pub fn loading(c: &mut Criterion) {
    let doc = fs::read_to_string("src/parser/tests/html/fr.serrer.html").unwrap();
    let html = HtmlDoc::parse(&doc).unwrap();
    let data = rkyv::to_bytes::<Error>(&html.dom()).unwrap();

    c.bench_function("loading fr.serrer.html", |b| {
        b.iter(|| {
            let _loaded = unsafe { rkyv::from_bytes_unchecked::<DomInner, Error>(&data).unwrap() };
        })
    });
}

criterion_group!(benches, loading,);
criterion_main!(benches);
