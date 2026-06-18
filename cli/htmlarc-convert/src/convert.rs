//! The `convert` command: parse a source's HTML documents into a single `.htmlarc` archive.
//!
//! A [`Source`](crate::source::Source) supplies bundle-sized runs; [`drive_runs_parallel`]
//! parses them on a worker pool and hands each completed run back in rank order, where the
//! coordinator writes its entries and seals it as one bundle. Per-document parse failures are
//! skipped and tallied (the strictness gap), never fatal.

use std::fmt::{self, Display};
use std::time::Instant;

use anyhow::Result;
use htmlarc_archive::{ArchiveWriter, HtmlEntry, SerializedEntry};
use htmlarc_dom::prelude::HtmlDoc;

use crate::args::Convert;
use crate::source::{DocSink, drive_runs_parallel, load_wordlist, open_source};

/// Builds one run's serialized archive entries, counting documents that fail to parse or
/// serialize. Serialization happens here, on the worker, so the heavy `DomInner` is dropped
/// the instant its bytes exist — a worker holds one live DOM, never a whole bundle's worth —
/// and the coordinator only ever appends already-compact bytes.
#[derive(Default)]
struct EntrySink {
    docs: Vec<SerializedEntry>,
    failed: u32,
}

impl DocSink for EntrySink {
    fn accept(&mut self, key: &str, html: &str) {
        // Per-doc: parse, build the DOM (optimal node width + checksum), and serialize it to
        // the on-disk form right away. The live `DomInner` is dropped before the next document.
        hotpath::measure_block!("convert::parse_doc", {
            match HtmlDoc::parse(html) {
                Ok(doc) => match HtmlEntry::new(key.to_string(), doc).into_serialized() {
                    Ok(ser) => self.docs.push(ser),
                    Err(e) => {
                        eprintln!("serialize failed for '{key}': {e}");
                        self.failed += 1;
                    }
                },
                Err(e) => {
                    eprintln!("parse failed for '{key}': {e}");
                    self.failed += 1;
                }
            }
        });
    }
}

/// One run's serialized bundle, handed to the coordinator.
struct BundleResult {
    docs: Vec<SerializedEntry>,
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
                docs: sink.docs,
                failed: sink.failed + read_failed,
            }
        },
        |result| {
            // Per-bundle: append every pre-serialized blob and stream it to disk. The heavy
            // work (parse + serialize) already happened on the worker; this is just writes.
            hotpath::measure_block!("convert::write_bundle", {
                if write_err.is_none() {
                    for doc in &result.docs {
                        if let Err(e) = writer.push_serialized(doc) {
                            write_err = Some(e.into());
                            break;
                        }
                        report.exported += 1;
                    }
                    // Seal the run as its own bundle (a no-op for an empty run), so on-disk
                    // bundles are exactly the source's runs. Sealing flushes the bundle's
                    // relocated string block, so only one run's text is buffered at a time.
                    if write_err.is_none()
                        && let Err(e) = writer.seal_bundle()
                    {
                        write_err = Some(e.into());
                    }
                }
            });
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
