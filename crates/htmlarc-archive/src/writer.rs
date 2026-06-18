//! The streaming archive writer.
//!
//! [`ArchiveWriter`] serializes and appends each document the moment it's pushed, then drops it.
//! Only a small in-RAM doc table (one [`DocEntry`] per doc), the per-bundle descriptors, and the
//! *current* bundle's relocated text survive across the pack, so peak RSS scales with one bundle,
//! never the whole corpus.
//!
//! On-disk result (bundle-segmented): each document's text/comment pool is relocated out of its
//! blob into its bundle's [`BundleStrings`] block, written right after that bundle's document
//! blobs:
//! `[header] ( [bundle doc blobs] [bundle string block] )* [doc table][bundle table][sort index][trailer]`.
//! Documents are grouped into [bundles](crate::DocBundle) **in arrival order**. A caller marks
//! each bundle boundary with [`seal_bundle`](ArchiveWriter::seal_bundle), which flushes that
//! bundle's accumulated string block and records its `(data_offset, data_len)` in the bundle
//! table; any unsealed tail is sealed by [`finish`](ArchiveWriter::finish). Callers that stream
//! without natural boundaries (a directory pack, a flat re-save) seal every [`BUNDLE_CAP`]
//! documents. Either way the boundaries are a deterministic function of the input, not of the
//! thread count. Every blob/region is 8-byte aligned (rkyv with `pointer_width_64` needs
//! 8-aligned starts). The file is written to a sibling temp path and atomically renamed on
//! `finish`, so an open `MmapArchive` over a previous build is never corrupted in place.

use std::collections::HashSet;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use fs_err as fs;
use fs_err::File;
use htmlarc_dom::prelude::HtmlDoc;
use rkyv::rancor::Error;

use crate::bundle::BundleDesc;
use crate::bundle_strings::BundleStrings;
use crate::doc_table::{self, DocEntry};
use crate::entry::{HtmlEntry, SerializedEntry};
use crate::error::ArchiveErr;
use crate::header::{HEADER_LEN, header_bytes};
use crate::trailer::Trailer;

const ALIGN: u64 = 8;

/// Streams documents into a `.htmlarc`, one blob at a time.
pub struct ArchiveWriter {
    out: BufWriter<File>,
    /// Running byte position in the file (header + blobs + padding). The single source of
    /// truth for every recorded offset — never derived from the `BufWriter`'s own position.
    pos: u64,
    /// One row per stored document, in arrival (== bundle→doc) order.
    docs: Vec<DocEntry>,
    /// One descriptor per sealed bundle, in order — built incrementally as each bundle is sealed
    /// (its string block is flushed at the same moment, so its `data_offset`/`data_len` are known).
    bundles: Vec<BundleDesc>,
    /// The current (not-yet-sealed) bundle's relocated text — one segment per document since the
    /// last seal. Flushed and reset by [`seal_bundle`](Self::seal_bundle).
    cur_strings: BundleStrings,
    /// Doc-table index at which the current bundle started.
    bundle_start_doc: usize,
    seen: HashSet<String>,
    collapsed: u64,
    tmp_path: PathBuf,
    final_path: PathBuf,
}

impl ArchiveWriter {
    /// Create a writer for `path`. Writes go to `path` + `.tmp` and are renamed into place by
    /// [`finish`](Self::finish).
    pub fn create<P: AsRef<Path>>(path: P) -> Result<Self, ArchiveErr> {
        let final_path = path.as_ref().to_path_buf();
        let mut tmp = final_path.clone().into_os_string();
        tmp.push(".tmp");
        let tmp_path = PathBuf::from(tmp);

        let file = File::create(&tmp_path).map_err(ArchiveErr::FileWrite)?;
        let mut out = BufWriter::new(file);
        out.write_all(&header_bytes())
            .map_err(ArchiveErr::FileWrite)?;

        Ok(Self {
            out,
            pos: HEADER_LEN as u64,
            docs: Vec::new(),
            bundles: Vec::new(),
            cur_strings: BundleStrings::default(),
            bundle_start_doc: 0,
            seen: HashSet::new(),
            collapsed: 0,
            tmp_path,
            final_path,
        })
    }

    /// Parse-built push: build the [`HtmlEntry`] (optimal node width + checksum) and append it.
    /// Duplicate keys are skipped (first wins) — the check happens *before* building so
    /// duplicates cost nothing.
    pub fn push(&mut self, key: String, html: HtmlDoc) -> Result<(), ArchiveErr> {
        if self.seen.contains(&key) {
            self.collapsed += 1;
            return Ok(());
        }
        let entry = HtmlEntry::new(key, html);
        self.push_entry(entry)
    }

    /// Append an already-built entry (used when re-saving an in-memory archive). Same first-wins
    /// dedup as [`push`](Self::push). Consumes the entry so its text pool can be relocated into
    /// the current bundle's string block, leaving the per-document blob string-less.
    pub fn push_entry(&mut self, mut entry: HtmlEntry) -> Result<(), ArchiveErr> {
        // Dedup before serializing so duplicates cost nothing.
        if !self.seen.insert(entry.key.clone()) {
            self.collapsed += 1;
            return Ok(());
        }
        let strings = entry.html.take_string_pool();
        let bytes =
            rkyv::to_bytes::<Error>(&entry).map_err(|e| ArchiveErr::Serialize(e.to_string()))?;
        self.write_doc(&entry.key, entry.key_len, entry.checksum, &bytes, &strings)
    }

    /// Append a document that was already serialized off-thread (the parallel `convert` path
    /// serializes each entry on its worker so the coordinator never holds the live `DomInner`).
    /// Its text pool was already relocated into [`SerializedEntry::strings`]. Same first-wins
    /// dedup as [`push_entry`](Self::push_entry).
    pub fn push_serialized(&mut self, entry: &SerializedEntry) -> Result<(), ArchiveErr> {
        if !self.seen.insert(entry.key.clone()) {
            self.collapsed += 1;
            return Ok(());
        }
        self.write_doc(
            &entry.key,
            entry.key_len,
            entry.checksum,
            &entry.bytes,
            &entry.strings,
        )
    }

    /// Append one document's (string-less) blob, record its doc-table row, and accumulate its
    /// relocated text into the current bundle's string block. The caller has already performed
    /// the dedup `seen` insert. The single source of byte offsets is `self.pos`.
    fn write_doc(
        &mut self,
        key: &str,
        key_len: u16,
        checksum: u64,
        bytes: &[u8],
        strings: &[u8],
    ) -> Result<(), ArchiveErr> {
        let offset = self.pos;
        let len = bytes.len() as u64;
        self.out.write_all(bytes).map_err(ArchiveErr::FileWrite)?;
        self.pos += len;
        self.pad_to_align()?;

        self.cur_strings.push_doc(strings);
        self.docs.push(DocEntry {
            key: key.to_string(),
            key_len,
            checksum,
            offset,
            len,
        });
        Ok(())
    }

    /// Write zero padding so the next write starts on an 8-byte boundary.
    fn pad_to_align(&mut self) -> Result<(), ArchiveErr> {
        let rem = self.pos % ALIGN;
        if rem != 0 {
            const ZEROS: [u8; ALIGN as usize] = [0u8; ALIGN as usize];
            let pad = (ALIGN - rem) as usize;
            self.out
                .write_all(&ZEROS[..pad])
                .map_err(ArchiveErr::FileWrite)?;
            self.pos += pad as u64;
        }
        debug_assert_eq!(self.pos % ALIGN, 0);
        Ok(())
    }

    /// Append a pre-serialized blob, padding to an 8-byte boundary first, and return its
    /// `(offset, unpadded_len)`. Padding before *every* region keeps each rkyv blob 8-aligned (a
    /// misaligned start corrupts `pointer_width_64` relative pointers).
    fn write_blob(&mut self, bytes: &[u8]) -> Result<(u64, u64), ArchiveErr> {
        self.pad_to_align()?;
        let offset = self.pos;
        let len = bytes.len() as u64;
        self.out.write_all(bytes).map_err(ArchiveErr::FileWrite)?;
        self.pos += len;
        Ok((offset, len))
    }

    /// Seal the current bundle: flush its accumulated string block into the file and record the
    /// bundle's doc range plus that block's `(data_offset, data_len)`. A seal with no documents
    /// since the last one is a no-op, so a run that parses to zero documents never creates an
    /// empty bundle. Callers that own whole bundles (the ZIM export's cluster-aligned runs) call
    /// this at each boundary; streaming callers call it every [`BUNDLE_CAP`](crate::BUNDLE_CAP)
    /// documents.
    pub fn seal_bundle(&mut self) -> Result<(), ArchiveErr> {
        let doc_count = self.docs.len() - self.bundle_start_doc;
        if doc_count == 0 {
            return Ok(());
        }
        let strings = std::mem::take(&mut self.cur_strings);
        let block =
            rkyv::to_bytes::<Error>(&strings).map_err(|e| ArchiveErr::Serialize(e.to_string()))?;
        let (data_offset, data_len) = self.write_blob(&block)?;

        self.bundles.push(BundleDesc {
            doc_start: self.bundle_start_doc as u32,
            doc_count: doc_count as u32,
            data_offset,
            data_len,
        });
        self.bundle_start_doc = self.docs.len();
        Ok(())
    }

    /// A permutation of doc-table positions ordered by `(key_len, key)` — the keyed-lookup index.
    /// Keys are unique (dedup above), so the order is total and the sort unambiguous.
    fn build_sort_index(&self) -> Vec<u32> {
        let mut perm: Vec<u32> = (0..self.docs.len() as u32).collect();
        perm.sort_by(|&a, &b| {
            let da = &self.docs[a as usize];
            let db = &self.docs[b as usize];
            doc_table::compare(da.key_len, &da.key, db.key_len, &db.key)
        });
        perm
    }

    /// Finalize: seal any unsealed tail bundle, then write the doc table (bundle order), the
    /// bundle table, and the sort index, then the trailer; flush and atomically rename.
    pub fn finish(mut self) -> Result<(), ArchiveErr> {
        // Flush the final, unsealed bundle (a streaming caller's tail, or the whole archive when
        // no one sealed explicitly).
        self.seal_bundle()?;

        let sort_index = self.build_sort_index();

        // The per-bundle string blocks are interleaved with the document blobs (each `BundleDesc`
        // records its own `data_offset`/`data_len`), so the trailer's data-region slot is
        // vestigial: record an empty region at the current position to keep its bounds valid.
        self.pad_to_align()?;
        let data_offset = self.pos;
        let data_len = 0u64;

        // The doc table is already in arrival (== bundle→doc) order.
        let doc_bytes = rkyv::to_bytes::<Error>(&self.docs)
            .map_err(|e| ArchiveErr::Serialize(e.to_string()))?;
        let bundle_bytes = rkyv::to_bytes::<Error>(&self.bundles)
            .map_err(|e| ArchiveErr::Serialize(e.to_string()))?;
        let sort_bytes = rkyv::to_bytes::<Error>(&sort_index)
            .map_err(|e| ArchiveErr::Serialize(e.to_string()))?;
        let (doc_table_offset, doc_table_len) = self.write_blob(&doc_bytes)?;
        let (bundle_table_offset, bundle_table_len) = self.write_blob(&bundle_bytes)?;
        let (sort_index_offset, sort_index_len) = self.write_blob(&sort_bytes)?;

        let trailer = Trailer {
            doc_table_offset,
            doc_table_len,
            bundle_table_offset,
            bundle_table_len,
            sort_index_offset,
            sort_index_len,
            data_offset,
            data_len,
            doc_count: self.docs.len() as u64,
            bundle_count: self.bundles.len() as u64,
        };
        self.out
            .write_all(&trailer.to_bytes())
            .map_err(ArchiveErr::FileWrite)?;
        self.out.flush().map_err(ArchiveErr::FileWrite)?;
        drop(self.out);

        fs::rename(&self.tmp_path, &self.final_path).map_err(ArchiveErr::FileWrite)?;
        Ok(())
    }

    /// Number of duplicate-keyed documents skipped so far.
    pub fn collapsed(&self) -> u64 {
        self.collapsed
    }

    /// Number of unique documents stored so far.
    pub fn doc_count(&self) -> usize {
        self.docs.len()
    }
}
