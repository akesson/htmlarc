//! The `convert` command: parse a source's HTML documents into a single `.htmlarc` archive.
//!
//! A [`Source`](crate::source::Source) supplies bundle-sized runs. Conversion is **two-phase**
//! (ADR 0005): a single-threaded *warm-up* parses the first runs and trains one archive-wide
//! string-compression dictionary from their text, then the rest stream through
//! [`drive_runs_parallel`] where each worker parses, serializes, and **compresses** its run's text
//! against the shared dictionary ("a bundle per core") before the coordinator writes it in rank
//! order and seals it as one bundle. Per-document parse/serialize failures are skipped and tallied
//! (the strictness gap), never fatal.

use std::fmt::{self, Display};
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use htmlarc_archive::{
    ArchiveWriter, HtmlEntry, MetaTableBuilder, SerializedEntry, StringCompressor, StringEncoder,
    train_string_dict,
};
use htmlarc_dom::prelude::HtmlDoc;

use crate::args::Convert;
use crate::source::{DocSink, MetaRow, drive_runs_parallel, load_wordlist, open_source};

/// Documents to gather before training the dictionary. The harness study (ADR 0005) found the
/// ratio plateaus by ~500 documents; ~1–2 bundles is comfortably past the knee while keeping the
/// warm-up buffer (compact serialized entries, the live DOMs already dropped) small.
const DICT_SAMPLE_DOCS: usize = 2_000;

/// Builds one run's serialized archive entries, counting documents that fail to parse or
/// serialize. Serialization happens here, on the worker, so the heavy `DomInner` is dropped
/// the instant its bytes exist — a worker holds one live DOM, never a whole bundle's worth.
/// When a `compressor` is present (the streaming phase), each document's relocated text is also
/// compressed into its frame here, so the coordinator only ever appends already-compact bytes;
/// the warm-up phase runs with no compressor (the dictionary does not exist yet) and emits raw
/// text for training, which the coordinator compresses once the dictionary is ready.
struct EntrySink<'a> {
    docs: Vec<SerializedEntry>,
    /// `rows[i]` is `docs[i]`'s metadata row (pushed together, so a parse/serialize failure
    /// drops both and the pairing survives to the coordinator).
    rows: Vec<Option<MetaRow>>,
    failed: u32,
    compressor: Option<StringCompressor<'a>>,
}

impl<'a> EntrySink<'a> {
    fn new(compressor: Option<StringCompressor<'a>>) -> Self {
        Self {
            docs: Vec::new(),
            rows: Vec::new(),
            failed: 0,
            compressor,
        }
    }
}

impl DocSink for EntrySink<'_> {
    fn accept(&mut self, key: &str, html: &str, meta: Option<MetaRow>) {
        // Per-doc: parse, build the DOM (optimal node width + checksum), and serialize it to
        // the on-disk form right away. The live `DomInner` is dropped before the next document.
        hotpath::measure_block!("convert::parse_doc", {
            let mut ser = match HtmlDoc::parse(html) {
                Ok(doc) => match HtmlEntry::new(key.to_string(), doc).into_serialized() {
                    Ok(ser) => ser,
                    Err(e) => {
                        eprintln!("serialize failed for '{key}': {e}");
                        self.failed += 1;
                        return;
                    }
                },
                Err(e) => {
                    eprintln!("parse failed for '{key}': {e}");
                    self.failed += 1;
                    return;
                }
            };
            if let Some(compressor) = &mut self.compressor
                && let Err(e) = ser.compress_blocks(compressor)
            {
                eprintln!("compress failed for '{key}': {e}");
                self.failed += 1;
                return;
            }
            self.docs.push(ser);
            self.rows.push(meta);
        });
    }
}

/// One run's serialized bundle, handed to the coordinator.
struct BundleResult {
    docs: Vec<SerializedEntry>,
    rows: Vec<Option<MetaRow>>,
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
    // When the source supplies per-document metadata (WARC: fetch date + status), accumulate
    // rows in the exact order documents are stored; `finish()` validates the 1:1 alignment.
    let mut meta = source.meta_schema().map(MetaTableBuilder::new);
    let mut report = Report {
        prepared: source.stats().prepared,
        ignored: source.stats().ignored,
        collapsed: source.stats().collapsed,
        exported: 0,
        failed: 0,
        start: Instant::now(),
    };

    let run_count = source.run_count();

    // ---- Phase 1: warm-up. Parse the first runs single-threaded, buffering their (raw) entries
    // until we have enough text to train the dictionary, or the input is exhausted. The buffer
    // holds compact serialized entries (live DOMs already dropped), so peak memory stays bounded.
    let mut warmup: Vec<BundleResult> = Vec::new();
    let mut warmup_doc_count = 0usize;
    let mut warmup_runs = 0usize;
    while warmup_runs < run_count && warmup_doc_count < DICT_SAMPLE_DOCS {
        let mut sink = EntrySink::new(None);
        let read_failed = source.drive_run(warmup_runs, &mut sink);
        warmup_doc_count += sink.docs.len();
        warmup.push(BundleResult {
            docs: sink.docs,
            rows: sink.rows,
            failed: sink.failed + read_failed,
        });
        warmup_runs += 1;
    }

    // ---- Phase 2: train one archive-wide dictionary from the warm-up text (single-threaded).
    // `None` (too little text) simply means dictionary-less compression — still a valid archive.
    let samples: Vec<&[u8]> = warmup
        .iter()
        .flat_map(|run| run.docs.iter().map(|d| d.strings.as_slice()))
        .collect();
    let dict = train_string_dict(&samples);
    drop(samples);
    let encoder = Arc::new(StringEncoder::new(dict.as_deref()));
    writer.set_dictionary(dict);

    // Store one pre-serialized doc and, when it is actually stored (not a writer-level key
    // duplicate), its metadata row — keeping the row stream 1:1 with the stored documents.
    fn store(
        writer: &mut ArchiveWriter,
        report: &mut Report,
        meta: &mut Option<MetaTableBuilder>,
        doc: &SerializedEntry,
        row: Option<MetaRow>,
    ) -> Result<()> {
        if writer.push_serialized(doc)? {
            report.exported += 1;
            if let Some(mb) = meta {
                let row = row.unwrap_or_else(|| vec![None; mb.schema().fields.len()]);
                mb.push_row(row)?;
            }
        }
        Ok(())
    }

    // ---- Phase 3: compress + write the buffered warm-up runs (coordinator, one compressor).
    {
        let mut compressor = encoder.compressor()?;
        for run in warmup {
            for (mut doc, row) in run.docs.into_iter().zip(run.rows) {
                doc.compress_blocks(&mut compressor)?;
                store(&mut writer, &mut report, &mut meta, &doc, row)?;
            }
            writer.seal_bundle()?;
            report.failed += run.failed;
        }
    }

    // ---- Phase 4: stream the remaining runs in parallel. Each worker compresses its run's text
    // against the shared dictionary ("a bundle per core"); the coordinator only writes + seals.
    let mut write_err: Option<anyhow::Error> = None;
    drive_runs_parallel(
        warmup_runs..run_count,
        |rank| {
            let compressor = encoder
                .compressor()
                .expect("building a per-worker compressor cannot fail");
            let mut sink = EntrySink::new(Some(compressor));
            let read_failed = source.drive_run(rank, &mut sink);
            BundleResult {
                docs: sink.docs,
                rows: sink.rows,
                failed: sink.failed + read_failed,
            }
        },
        |result| {
            // Per-bundle: append every pre-serialized, pre-compressed blob and stream it to disk.
            // The heavy work (parse + serialize + compress) already happened on the worker.
            hotpath::measure_block!("convert::write_bundle", {
                if write_err.is_none() {
                    for (doc, row) in result.docs.iter().zip(result.rows) {
                        if let Err(e) = store(&mut writer, &mut report, &mut meta, doc, row) {
                            write_err = Some(e);
                            break;
                        }
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
    writer.set_meta_table(meta.map(MetaTableBuilder::finish));
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
