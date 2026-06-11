//! The `convert` command: parse a source's HTML documents into a single `.htmlarc` archive.
//!
//! A [`Source`](crate::source::Source) supplies bundle-sized runs; [`drive_runs_parallel`]
//! parses them on a worker pool and hands each completed run back in rank order, where the
//! coordinator writes its entries and seals it as one bundle. Per-document parse failures are
//! skipped and tallied (the strictness gap), never fatal.

use std::fmt::{self, Display};
use std::time::Instant;

use anyhow::Result;
use htmlarc_archive::{ArchiveWriter, HtmlEntry};
use htmlarc_dom::prelude::HtmlDoc;

use crate::args::Convert;
use crate::source::{DocSink, drive_runs_parallel, load_wordlist, open_source};

/// Builds one run's archive entries, counting documents that fail to parse.
#[derive(Default)]
struct EntrySink {
    entries: Vec<HtmlEntry>,
    parse_failed: u32,
}

impl DocSink for EntrySink {
    fn accept(&mut self, key: &str, html: &str) {
        match HtmlDoc::parse(html) {
            // Build the HtmlEntry here (optimal node width + checksum) so the heavy
            // per-document work happens on the worker thread.
            Ok(doc) => self.entries.push(HtmlEntry::new(key.to_string(), doc)),
            Err(e) => {
                eprintln!("parse failed for '{key}': {e}");
                self.parse_failed += 1;
            }
        }
    }
}

/// One run's parsed bundle, handed to the coordinator.
struct BundleResult {
    entries: Vec<HtmlEntry>,
    failed: u32,
}

/// Tally + timing for a conversion.
struct Report {
    prepared: usize,
    ignored: u32,
    collapsed: u32,
    exported: u32,
    failed: u32,
    start: Instant,
}

impl Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "Prepared: {}\nConverted: {}\nFailed: {}\nIgnored: {}",
            self.prepared, self.exported, self.failed, self.ignored
        )?;
        let d = self.start.elapsed();
        writeln!(
            f,
            "Time elapsed: {}m {}s",
            d.as_secs() / 60,
            d.as_secs() % 60
        )
    }
}

pub(crate) fn run(args: Convert) -> Result<()> {
    let Convert {
        input,
        output,
        list,
        limit,
        format,
    } = args;

    let wordlist = load_wordlist(list.as_deref())?;
    let source = open_source(&input, format.as_deref(), wordlist.as_ref(), limit)?;
    let source = source.as_ref();

    let mut writer = ArchiveWriter::create(&output)?;
    let mut report = Report {
        prepared: source.stats().prepared,
        ignored: source.stats().ignored,
        collapsed: source.stats().collapsed,
        exported: 0,
        failed: 0,
        start: Instant::now(),
    };
    let mut write_err: Option<anyhow::Error> = None;

    drive_runs_parallel(
        source.run_count(),
        |rank| {
            let mut sink = EntrySink::default();
            let read_failed = source.drive_run(rank, &mut sink);
            BundleResult {
                entries: sink.entries,
                failed: sink.parse_failed + read_failed,
            }
        },
        |result| {
            if write_err.is_none() {
                for entry in &result.entries {
                    if let Err(e) = writer.push_entry(entry) {
                        write_err = Some(e.into());
                        break;
                    }
                    report.exported += 1;
                }
                // Seal the run as its own bundle (a no-op for an empty run), so on-disk
                // bundles are exactly the source's runs.
                if write_err.is_none() {
                    writer.seal_bundle();
                }
            }
            report.failed += result.failed;
        },
    );

    if let Some(e) = write_err {
        return Err(e);
    }

    let stored = writer.doc_count() as u32;
    writer.finish()?;

    print!("{report}");
    if report.collapsed > 0 {
        println!(
            "Note: {} document(s) shared a key and were merged ({stored} unique in archive).",
            report.collapsed
        );
    }
    Ok(())
}
