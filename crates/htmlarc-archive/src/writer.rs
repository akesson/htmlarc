//! The streaming archive writer.
//!
//! [`ArchiveWriter`] serializes and appends each document the moment it's pushed, then drops it.
//! Only a small in-RAM doc table (one [`DocEntry`] per doc) plus a dedup key-set survive across
//! the pack, so peak RSS never scales with the whole corpus.
//!
//! On-disk result (v4, bundle-segmented):
//! `[header][doc blob]…[per-bundle data region][doc table][bundle table][sort index][trailer]`.
//! Documents are grouped into [bundles](crate::DocBundle) **in arrival order**. A caller that
//! owns whole bundles (the ZIM export, which produces cluster-aligned runs) marks each boundary
//! with [`seal_bundle`](ArchiveWriter::seal_bundle); a caller that just streams documents (a
//! directory pack, a re-save of a flat list) seals nothing and the bundle table falls back to
//! chunking the doc table into [`BUNDLE_CAP`]-sized runs at [`finish`](ArchiveWriter::finish).
//! Either way the boundaries are a deterministic function of the input, not of the thread count.
//! Every blob/region is 8-byte aligned (rkyv with `pointer_width_64` needs 8-aligned starts).
//! The file is written to a sibling temp path and atomically renamed on `finish`, so an open
//! `MmapArchive` over a previous build is never corrupted in place.

use std::collections::HashSet;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use fs_err as fs;
use fs_err::File;
use htmlarc_dom::prelude::HtmlDoc;
use rkyv::rancor::Error;

use crate::bundle::{BUNDLE_CAP, BundleDesc};
use crate::doc_table::{self, DocEntry};
use crate::entry::{HtmlEntry, SerializedEntry};
use crate::error::ArchiveErr;
use crate::header::{HEADER_LEN, header_bytes};
use crate::trailer::Trailer;

const ALIGN: u64 = 8;

/// Streams documents into a v4 `.htmlarc`, one blob at a time.
pub struct ArchiveWriter {
    out: BufWriter<File>,
    /// Running byte position in the file (header + blobs + padding). The single source of
    /// truth for every recorded offset — never derived from the `BufWriter`'s own position.
    pos: u64,
    /// One row per stored document, in arrival (== bundle→doc) order.
    docs: Vec<DocEntry>,
    /// Doc-table index at which each sealed bundle *ends* (ascending). Empty when the caller
    /// never calls [`seal_bundle`](Self::seal_bundle), in which case the bundle table is derived
    /// by chunking at [`BUNDLE_CAP`].
    bundle_ends: Vec<usize>,
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
            bundle_ends: Vec::new(),
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
        self.push_entry(&entry)
    }

    /// Append an already-built entry (used when re-saving an in-memory archive, and by the
    /// parallel ZIM export which builds [`HtmlEntry`]s off-thread). Same first-wins dedup as
    /// [`push`](Self::push). Serializes on the calling thread.
    pub fn push_entry(&mut self, entry: &HtmlEntry) -> Result<(), ArchiveErr> {
        // Dedup before serializing so duplicates cost nothing.
        if !self.seen.insert(entry.key.clone()) {
            self.collapsed += 1;
            return Ok(());
        }
        let bytes =
            rkyv::to_bytes::<Error>(entry).map_err(|e| ArchiveErr::Serialize(e.to_string()))?;
        self.write_doc(&entry.key, entry.key_len, entry.checksum, &bytes)
    }

    /// Append a document that was already serialized off-thread (the parallel `convert` path
    /// serializes each entry on its worker so the coordinator never holds the live `DomInner`).
    /// Same first-wins dedup as [`push_entry`](Self::push_entry); the serialization is already
    /// done, so a duplicate just discards the bytes.
    pub fn push_serialized(&mut self, entry: &SerializedEntry) -> Result<(), ArchiveErr> {
        if !self.seen.insert(entry.key.clone()) {
            self.collapsed += 1;
            return Ok(());
        }
        self.write_doc(&entry.key, entry.key_len, entry.checksum, &entry.bytes)
    }

    /// Append one document's serialized blob and record its doc-table row. The caller has
    /// already performed the dedup `seen` insert; this just writes the bytes (8-byte padded)
    /// and pushes the [`DocEntry`]. The single source of byte offsets is `self.pos`.
    fn write_doc(
        &mut self,
        key: &str,
        key_len: u16,
        checksum: u64,
        bytes: &[u8],
    ) -> Result<(), ArchiveErr> {
        let offset = self.pos;
        let len = bytes.len() as u64;
        self.out.write_all(bytes).map_err(ArchiveErr::FileWrite)?;
        self.pos += len;
        self.pad_to_align()?;

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

    /// Append a pre-serialized footer blob, padding to an 8-byte boundary first, and return its
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

    /// Mark the end of a bundle at the current document count. Called by a caller that owns whole
    /// bundles (the ZIM export, after a cluster-aligned run) so the on-disk bundle boundaries
    /// match the runs the worker produced. An empty seal (no documents since the last one) is a
    /// no-op, so a run that parses to zero documents never creates an empty bundle.
    pub fn seal_bundle(&mut self) {
        let end = self.docs.len();
        if end > self.bundle_ends.last().copied().unwrap_or(0) {
            self.bundle_ends.push(end);
        }
    }

    /// Build the bundle table from the doc table. When the caller sealed explicit boundaries (the
    /// ZIM export's cluster-aligned runs) those are used verbatim, with any unsealed tail forming
    /// a final bundle; otherwise the doc table is chunked into [`BUNDLE_CAP`]-sized bundles. The
    /// data slot is reserved (0/0) for now.
    fn build_bundle_table(&self) -> Vec<BundleDesc> {
        let total = self.docs.len();
        let ends: Vec<usize> = if self.bundle_ends.is_empty() {
            (1..=total.div_ceil(BUNDLE_CAP))
                .map(|k| (k * BUNDLE_CAP).min(total))
                .collect()
        } else {
            let mut ends = self.bundle_ends.clone();
            // A caller might push documents after its last seal; bundle the tail too.
            if ends.last().copied().unwrap_or(0) < total {
                ends.push(total);
            }
            ends
        };

        let mut bundles = Vec::with_capacity(ends.len());
        let mut start = 0usize;
        for end in ends {
            if end == start {
                continue;
            }
            bundles.push(BundleDesc {
                doc_start: start as u32,
                doc_count: (end - start) as u32,
                data_offset: 0,
                data_len: 0,
            });
            start = end;
        }
        bundles
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

    /// Finalize: write the (empty) per-bundle data region, the doc table (bundle order), the
    /// bundle table, and the sort index, then the trailer; flush and atomically rename.
    pub fn finish(mut self) -> Result<(), ArchiveErr> {
        let bundles = self.build_bundle_table();
        let sort_index = self.build_sort_index();

        // Per-bundle data region: empty for now, but keep its start 8-aligned and recorded so a
        // future per-bundle-data step can populate it without another format bump.
        self.pad_to_align()?;
        let data_offset = self.pos;
        let data_len = 0u64;

        // The doc table is already in arrival (== bundle→doc) order.
        let doc_bytes = rkyv::to_bytes::<Error>(&self.docs)
            .map_err(|e| ArchiveErr::Serialize(e.to_string()))?;
        let bundle_bytes =
            rkyv::to_bytes::<Error>(&bundles).map_err(|e| ArchiveErr::Serialize(e.to_string()))?;
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
            bundle_count: bundles.len() as u64,
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
