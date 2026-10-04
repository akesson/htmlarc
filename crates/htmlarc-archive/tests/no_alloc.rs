//! Sweeping an archive with a selector must not touch the heap per document: binding a [`Doc`]
//! borrows its block tables and keeps its inflate caches inline, and `select(&mut list)`
//! re-resolves one selector list in place. Allocation is what scales worst across a parallel
//! sweep's threads (the system allocator contends), so this pins it at zero.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use htmlarc_archive::{HtmlArchiveBuilder, MmapArchive};
use htmlarc_dom::prelude::{DomRead, HtmlDoc, OwnedSelectorList};

/// Counts this thread's allocations, so the test harness's own threads don't interfere.
struct Counting;

thread_local! {
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.with(|n| n.set(n.get() + 1));
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn allocs() -> usize {
    ALLOCS.with(Cell::get)
}

#[test]
fn selector_sweep_allocates_nothing_per_document() {
    let mut b = HtmlArchiveBuilder::default();
    for i in 0..20 {
        let html = format!(
            r#"<body><h1 class="t">doc {i}</h1><p class="c x"><a href="/{i}">link</a> text</p></body>"#
        );
        b.add_html(format!("doc{i}"), HtmlDoc::parse(&html).unwrap());
    }
    b.add_html(
        "empty".to_string(),
        HtmlDoc::parse("<body></body>").unwrap(),
    );
    let path = std::env::temp_dir().join(format!("htmlarc_noalloc_{}.htmlarc", std::process::id()));
    b.build().write_to(&path).unwrap();
    let archive = MmapArchive::open(&path).unwrap();

    for css in ["a[href]", "p.c a", "h1, h2, h3", ".missing", "clipPath"] {
        let selector = OwnedSelectorList::parse(css).unwrap();
        let mut list = selector.list().clone();
        let sweep = |list: &mut _| -> usize {
            (0..archive.len())
                .map(|i| {
                    archive
                        .try_doc(i)
                        .unwrap()
                        .root()
                        .select(&mut *list)
                        .count()
                })
                .sum()
        };
        // First pass validates each bundle's string block once (memoized per bundle).
        let expected = sweep(&mut list);

        let before = allocs();
        let total = sweep(&mut list);
        assert_eq!(allocs() - before, 0, "{css}: per-document allocations");
        assert_eq!(
            total, expected,
            "{css}: re-resolved list matches identically"
        );

        // The counter does see allocations: a per-document clone of the list allocates.
        let before = allocs();
        let cloned: usize = (0..archive.len())
            .map(|i| {
                archive
                    .try_doc(i)
                    .unwrap()
                    .root()
                    .select(list.clone())
                    .count()
            })
            .sum();
        assert!(allocs() > before, "{css}: counter sees the clones");
        assert_eq!(
            cloned, expected,
            "{css}: cloned and re-resolved lists agree"
        );
    }
    std::fs::remove_file(&path).ok();
}
