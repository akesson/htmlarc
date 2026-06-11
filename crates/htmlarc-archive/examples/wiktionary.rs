//! # Wiktionary step-wise reduction — a worked `htmlarc` example
//!
//! This mirrors how [`polyglot-ng`](https://github.com/) uses htmlarc to turn a raw
//! Wiktionary dump into compact, query-ready data: a **pipeline of DOM→DOM reduction
//! passes**, where a `.htmlarc` archive is the durable checkpoint between every stage.
//! Each pass reads the previous `stepN.htmlarc`, mutates every document's DOM in place,
//! re-`repackage()`s it (compacting the flat node arena and string pool), and writes the
//! next archive. Because each stage is a real file, reductions are resumable and
//! inspectable (`htmlarc list`, `htmlarc diff`).
//!
//! The stages here, over two real Swedish Wiktionary pages (mobile MediaWiki skin):
//!
//! ```text
//!   examples/wiktionary/*.html
//!        │  step0 — pack raw HTML directory into an archive   (HtmlArchive::pack_to)
//!        ▼
//!   step0.htmlarc   full pages: scripts, RLCONF blob, nav chrome, footer …
//!        │  step1 — clean <head> + triage out "no-entry" pages
//!        ▼
//!   step1.htmlarc   minimal <head> (charset + title); body chrome still present
//!        │  step2 — lift the article (div.mw-parser-output) up to <body>
//!        ▼
//!   step2.htmlarc   just the dictionary content + a license marker
//!        │  query — reopen zero-copy and CSS-select, no re-parse   (MmapArchive)
//!        ▼
//!   printed report: per-step byte sizes (shrinking), diff counts, sample definitions
//! ```
//!
//! The example asserts its own invariants as it goes, so running it *is* the test:
//!
//! ```sh
//! cargo run -p htmlarc-archive --example wiktionary
//! ```

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use htmlarc_archive::{HtmlArchive, HtmlArchiveBuilder, HtmlEntry, MmapArchive};
use htmlarc_dom::prelude::*;
use htmlarc_macros::css;

fn main() -> Result<()> {
    let data_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/wiktionary");
    let out_dir = std::env::temp_dir().join(format!("htmlarc-wiktionary-{}", std::process::id()));
    std::fs::create_dir_all(&out_dir)?;

    let step0 = out_dir.join("step0.htmlarc");
    let step1 = out_dir.join("step1.htmlarc");
    let step2 = out_dir.join("step2.htmlarc");

    // ---- step 0: pack a directory of raw HTML straight into an archive --------------
    // The high-level streaming packer parses each *.html file and keys it by file stem,
    // holding at most one document in memory at a time.
    let raw_count = HtmlArchive::pack_to(&data_dir, &step0)?;
    println!(
        "step0  packed {raw_count} raw page(s) → {}",
        step0.display()
    );
    assert_eq!(raw_count, 3, "expected the 3 fixture pages");

    // ---- step 1: clean <head> and triage out non-entries ----------------------------
    let skipped = reduce_clean_head(&step0, &step1)?;
    println!(
        "step1  cleaned heads, skipped {skipped} non-entry page(s) → {}",
        step1.display()
    );
    assert_eq!(
        skipped, 1,
        "the synthetic `template-inget_uppslag` page should be skipped"
    );
    assert_head_is_minimal(&step1, "aerodynamik")?;

    // ---- step 2: extract the article body --------------------------------------------
    reduce_extract_content(&step1, &step2)?;
    println!("step2  extracted article content → {}", step2.display());

    // ---- query the final archive, zero-copy ------------------------------------------
    println!("\nsample definitions queried from the reduced archive (no re-parse):");
    query_definitions(&step2)?;

    // ---- report: sizes shrink at every checkpoint, and every doc changed -------------
    let (s0, s1, s2) = (file_len(&step0)?, file_len(&step1)?, file_len(&step2)?);
    println!("\narchive sizes:");
    println!("  step0 {:>8} B  (raw)", s0);
    println!(
        "  step1 {:>8} B  ({:.0}% of step0, head cleaned)",
        s1,
        pct(s1, s0)
    );
    println!(
        "  step2 {:>8} B  ({:.0}% of step0, content only)",
        s2,
        pct(s2, s0)
    );
    assert!(s1 < s0, "step1 should be smaller than step0 ({s1} !< {s0})");
    assert!(s2 < s1, "step2 should be smaller than step1 ({s2} !< {s1})");

    println!("\nper-step diff (documents whose checksum changed):");
    println!("  step0 → step1: {} doc(s)", changed_docs(&step0, &step1)?);
    println!("  step1 → step2: {} doc(s)", changed_docs(&step1, &step2)?);

    println!(
        "\nartifacts left in {} (inspect with the `htmlarc` CLI)",
        out_dir.display()
    );
    Ok(())
}

/// Step 1: for every document, drop "no-entry" placeholders, strip presentational
/// classes off `<html>`/`<body>`, and reduce `<head>` to just charset + title.
///
/// Returns the number of documents skipped. This is the read-checkpoint → mutate →
/// write-checkpoint loop that defines the pipeline.
fn reduce_clean_head(input: &Path, output: &Path) -> Result<usize> {
    let archive = HtmlArchive::read_from(input)?;
    let mut builder = HtmlArchiveBuilder::default();
    let mut skipped = 0usize;

    for HtmlEntry { key, html, .. } in archive.into_entries() {
        // A parsed, owned document made mutable. (`DomRefCell` is interior-mutable, so
        // the `HtmlElement` handles below can edit the tree through shared borrows.)
        let dom = DomRefCell::new(html);

        // Triage: Swedish Wiktionary marks "no such entry" pages with this template.
        if dom
            .root()
            .select(css!("body p.template-inget_uppslag"))
            .next()
            .is_some()
        {
            skipped += 1;
            continue;
        }

        clean_head_and_classes(&dom, &key)?;

        // Compact the mutated arena and store the result in the next checkpoint.
        builder.add_html(key, HtmlDoc::from(dom.repackage()));
    }

    builder.write_to(output)?;
    Ok(skipped)
}

/// Strip `<html>`/`<body>` classes and rebuild `<head>` as `<meta charset>` + `<title>`.
fn clean_head_and_classes(dom: &DomRefCell, word: &str) -> Result<()> {
    let html = dom
        .root()
        .first_child_tag(HtmlTag::html)
        .ok()
        .ok_or_else(|| anyhow!("{word}: no <html> element"))?;
    html.classes_mut().remove(|_| true);

    let head = html
        .first_child_tag(HtmlTag::head)
        .ok()
        .ok_or_else(|| anyhow!("{word}: no <head> element"))?;

    // Preserve the declared charset before discarding the head's contents.
    let charset = head
        .select(css!("meta[charset]"))
        .first()
        .ok()
        .and_then(|m| m.attribute(HtmlAttr::charset))
        .unwrap_or_else(|| "UTF-8".to_string());

    head.remove_children();
    head.prepend_child(HtmlTag::meta)
        .attributes_mut()
        .append(Attribute {
            name: AttrName::Std(HtmlAttr::charset),
            val: &charset,
        });
    head.append_child(HtmlTag::title).append_text_child(word);

    html.first_child_tag(HtmlTag::body)
        .ok()
        .ok_or_else(|| anyhow!("{word}: no <body> element"))?
        .classes_mut()
        .remove(|_| true);

    Ok(())
}

/// Step 2: lift `div.mw-parser-output` (the article) up to be `<body>`'s content,
/// discarding the navigation/header/footer chrome, and append a license marker.
fn reduce_extract_content(input: &Path, output: &Path) -> Result<()> {
    let archive = HtmlArchive::read_from(input)?;
    let mut builder = HtmlArchiveBuilder::default();

    for HtmlEntry { key, html, .. } in archive.into_entries() {
        let dom = DomRefCell::new(html);
        extract_main_content(&dom, &key)?;
        builder.add_html(key, HtmlDoc::from(dom.repackage()));
    }

    builder.write_to(output)?;
    Ok(())
}

fn extract_main_content(dom: &DomRefCell, word: &str) -> Result<()> {
    let body = dom
        .root()
        .path([HtmlTag::html, HtmlTag::body])
        .ok()
        .ok_or_else(|| anyhow!("{word}: no <body> element"))?;

    // Sanity-check this is a CC-licensed Wiktionary page. The exact license URL varies
    // between dumps, so key off the footer `div.license` block rather than an href.
    if body.select(css!("footer div.license")).next().is_none() {
        return Err(anyhow!(
            "{word}: no license block — not a recognized Wiktionary page"
        ));
    }

    // Walk up from the article to <body>, one level per iteration: remove the off-spine
    // siblings at the current level, then `unwrap` the spine node so the article rises
    // into its grandparent. `unwrap_element`/`remove` auto-prune the emptied ancestors.
    while let Ok(article) = body.select(css!("div.mw-parser-output")).first() {
        let article_idx = article.index();
        let Ok(parent) = article.parent() else { break };
        if parent.tag() == HtmlTag::body {
            break;
        }
        let off_spine: Vec<_> = parent
            .children()
            .filter(|c| c.index() != article_idx)
            .collect();
        for sibling in off_spine {
            sibling.remove();
        }
        parent.unwrap_element();
    }

    // Drop everything left directly under <body> that isn't the article — including the
    // indentation whitespace and stray comments lifted up by the unwraps above (children()
    // skips text/comment nodes unless asked to include them).
    if let Ok(article) = body.select(css!("div.mw-parser-output")).first() {
        let keep = article.index();
        let extras: Vec<_> = body
            .children()
            .set_include_text()
            .set_include_comment()
            .filter(|c| c.index() != keep)
            .collect();
        for extra in extras {
            extra.remove();
        }
    }

    // A normalized license marker, mirroring the polyglot step.
    body.append_child(HtmlTag::h2).append_text_child("Licens");
    Ok(())
}

/// Assert step 1 reduced `<head>` to exactly two elements (`<meta>` + `<title>`).
fn assert_head_is_minimal(archive_path: &Path, key: &str) -> Result<()> {
    let archive = HtmlArchive::read_from(archive_path)?;
    assert_eq!(
        archive.len(),
        2,
        "step1 should hold the 2 surviving entries"
    );

    let entry = archive
        .get(key)
        .ok_or_else(|| anyhow!("{key} missing from step1"))?;
    let head = entry
        .root()
        .path([HtmlTag::html, HtmlTag::head])
        .ok()
        .ok_or_else(|| anyhow!("{key}: no <head>"))?;
    let children = head.children().count();
    assert_eq!(
        children, 2,
        "{key}: head should be <meta>+<title>, found {children} nodes"
    );
    Ok(())
}

/// Reopen the final archive zero-copy and CSS-select a sample definition from each entry,
/// asserting the article content survived the reduction.
fn query_definitions(archive_path: &Path) -> Result<()> {
    let mmap = MmapArchive::open(archive_path)?;
    for entry in mmap.entries() {
        let key = entry.key();

        // The article must still be present and non-empty after reduction.
        let article = entry
            .root()
            .select_css("div.mw-parser-output")
            .ok()
            .and_then(|mut m| m.first().ok())
            .ok_or_else(|| anyhow!("{key}: article content lost during reduction"))?;
        let text = article.text_content();
        assert!(
            !text.trim().is_empty(),
            "{key}: article text empty after reduction"
        );

        // Pull the first numbered definition (`<ol><li>`) as a human-readable sample,
        // falling back to any list item or paragraph.
        let first_match = |sel: &'static str| {
            entry
                .root()
                .select_css(sel)
                .ok()
                .and_then(|mut m| m.first().ok())
        };
        let sample = first_match("ol li")
            .or_else(|| first_match("li, p"))
            .map(|el| el.text_content())
            .unwrap_or_else(|| text.clone());
        println!("  {key:>14}  {}", snippet(&sample, 90));
    }
    Ok(())
}

fn changed_docs(a: &Path, b: &Path) -> Result<usize> {
    let (a, b) = (MmapArchive::open(a)?, MmapArchive::open(b)?);
    Ok(b.keys()
        .filter(|k| a.checksum_for_key(k) != b.checksum_for_key(k))
        .count())
}

fn file_len(path: &Path) -> Result<u64> {
    Ok(std::fs::metadata(path)?.len())
}

fn pct(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64 * 100.0
    }
}

fn snippet(text: &str, max: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > max {
        format!("{}…", collapsed.chars().take(max).collect::<String>())
    } else {
        collapsed
    }
}
