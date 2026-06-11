use std::collections::HashSet;
use std::path::PathBuf;

use htmlarc_archive::BUNDLE_CAP;
use zim::{MimeType, Namespace};

use crate::source::parse_wordlist;
use crate::source::zim::{group_into_runs, html_mime, is_content, key_for};

/// Build a cluster of `docs` html blobs (the keys are irrelevant to grouping).
fn cluster(idx: u32, docs: usize) -> (u32, Vec<(u32, String)>) {
    (
        idx,
        (0..docs)
            .map(|b| (b as u32, format!("k{idx}_{b}")))
            .collect(),
    )
}

#[test]
fn runs_are_cluster_aligned_and_about_bundle_cap() {
    // Clusters of ~186 docs (the measured median) are grouped into runs of >= BUNDLE_CAP each,
    // except the final remainder. Clusters are never split across runs.
    let per_cluster = 186;
    let cluster_count = 600; // ~111_600 docs -> ~11 runs
    let clusters: Vec<_> = (0..cluster_count)
        .map(|i| cluster(i, per_cluster))
        .collect();
    let total_docs: usize = clusters.iter().map(|(_, b)| b.len()).sum();

    let runs = group_into_runs(clusters);

    // Every run but the last reaches the cap; the last holds the remainder (<= cap + a cluster).
    assert!(runs.len() >= 2);
    for run in &runs[..runs.len() - 1] {
        let docs: usize = run.iter().map(|(_, b)| b.len()).sum();
        assert!(
            docs >= BUNDLE_CAP,
            "non-final run must reach the cap, got {docs}"
        );
        assert!(
            docs < BUNDLE_CAP + per_cluster,
            "must seal at the first cluster past the cap"
        );
    }

    // No documents are lost or duplicated, and clusters stay in ascending order across the runs.
    let regrouped: Vec<u32> = runs.iter().flatten().map(|(idx, _)| *idx).collect();
    assert_eq!(regrouped, (0..cluster_count).collect::<Vec<_>>());
    let regrouped_docs: usize = runs.iter().flatten().map(|(_, b)| b.len()).sum();
    assert_eq!(regrouped_docs, total_docs);
}

#[test]
fn oversized_cluster_forms_its_own_run() {
    // A single cluster larger than the cap can't be split — it becomes one run on its own.
    let clusters = vec![cluster(0, 5), cluster(1, BUNDLE_CAP + 500), cluster(2, 5)];
    let runs = group_into_runs(clusters);

    // [0+1] seal once 1 pushes past the cap; [2] is the trailing remainder.
    assert_eq!(runs.len(), 2);
    assert_eq!(
        runs[0].iter().map(|(i, _)| *i).collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_eq!(runs[1].iter().map(|(i, _)| *i).collect::<Vec<_>>(), vec![2]);
}

#[test]
fn empty_work_list_makes_no_runs() {
    assert!(group_into_runs(Vec::new()).is_empty());
}

#[test]
fn html_mime_accepts_only_text_html() {
    assert!(html_mime(&MimeType::Type("text/html".into())));
    assert!(html_mime(&MimeType::Type(
        "text/html; charset=utf-8".into()
    )));
    assert!(!html_mime(&MimeType::Type("image/png".into())));
    assert!(!html_mime(&MimeType::Type("text/css".into())));
    assert!(!html_mime(&MimeType::Redirect));
}

#[test]
fn content_namespaces_cover_both_schemes() {
    assert!(is_content(&Namespace::Articles)); // old scheme ('A')
    assert!(is_content(&Namespace::UserContent)); // new scheme ('C')
    assert!(!is_content(&Namespace::Metadata));
    assert!(!is_content(&Namespace::FulltextIndex));
}

#[test]
fn key_falls_back_to_url_when_title_empty() {
    assert_eq!(
        key_for("Climate change", "Climate_change"),
        "Climate change"
    );
    assert_eq!(key_for("", "Some_Slug"), "Some_Slug"); // empty title -> url slug
    assert_eq!(key_for("Cafe\u{0301}", "x"), "Caf\u{00e9}"); // still NFC-normalized
}

#[test]
fn wordlist_is_nfc_normalized_deduped_and_skips_blanks() {
    // "Cafe" + combining acute, a blank line, a duplicate, and a plain word.
    let set: HashSet<String> = parse_wordlist("Cafe\u{0301}\n\nCafe\u{0301}\nplain\n");
    assert!(set.contains("Caf\u{00e9}")); // normalized to precomposed
    assert!(set.contains("plain"));
    assert_eq!(set.len(), 2);
}

/// End-to-end against a real ZIM. Ignored by default because no `.zim` fixture is committed
/// (the openzim test suite is unlicensed and can't be redistributed here). To run it, fetch a
/// small ZIM first with `cli/htmlarc-convert/fetch-testdata.sh`, then:
///   cargo nextest run -p htmlarc-convert --run-ignored all
/// Override the ZIM path with the `HTMLARC_CONVERT_TEST_ZIM` env var.
#[test]
#[ignore = "needs a local ZIM; run cli/htmlarc-convert/fetch-testdata.sh first"]
fn convert_reads_a_real_zim() {
    let zim_path = std::env::var_os("HTMLARC_CONVERT_TEST_ZIM")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/test.zim"));
    assert!(
        zim_path.exists(),
        "no test ZIM at {} — set HTMLARC_CONVERT_TEST_ZIM or run cli/htmlarc-convert/fetch-testdata.sh",
        zim_path.display()
    );

    let out = std::env::temp_dir().join("htmlarc-convert-e2e.htmlarc");
    crate::convert::run(crate::args::Convert {
        input: zim_path,
        output: out.clone(),
        list: None,
        limit: None,
        format: None,
    })
    .expect("convert should succeed");

    let arch = htmlarc_archive::HtmlArchive::read_from(&out).expect("archive should load");
    assert!(!arch.is_empty(), "expected at least one article");
}
