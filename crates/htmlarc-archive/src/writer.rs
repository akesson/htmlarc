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
//! 8-aligned starts). A fresh archive ([`create`](ArchiveWriter::create)) is written to a
//! sibling temp path and atomically renamed on `finish`, so an open `MmapArchive` over a
//! previous build is never corrupted in place; [`append`](ArchiveWriter::append) instead
//! extends an existing file past its old footer under the header-staged recovery contract
//! (ADR 0010).

use std::collections::HashSet;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use fs_err as fs;
use fs_err::File;
use htmlarc_dom::prelude::HtmlDoc;
use rkyv::rancor::Error;

use zstd::bulk::Compressor;

use crate::bundle::BundleDesc;
use crate::bundle_strings::BundleStrings;
use crate::codec;
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
    /// The current (not-yet-sealed) bundle's relocated text — one compressed frame per document
    /// since the last seal. Flushed and reset by [`seal_bundle`](Self::seal_bundle).
    cur_strings: BundleStrings,
    /// Reused zstd context that compresses each document's text into its frame on the raw
    /// [`push_entry`](Self::push_entry) path (the streaming convert path supplies frames already
    /// compressed off-thread). Dictionary-less (ADR 0005).
    compressor: Compressor<'static>,
    /// The archive-wide string dictionary to record in the trailer, set by the two-phase convert
    /// path via [`set_dictionary`](Self::set_dictionary). `None` ⇒ frames were compressed
    /// dictionary-less, so an empty region (length 0) is written.
    dict: Option<Vec<u8>>,
    /// The complete typed metadata table (ADR 0009), set via [`set_meta_table`](Self::set_meta_table)
    /// before [`finish`](Self::finish). `None` ⇒ an empty region (length 0) is written. Rows must
    /// align 1:1 with stored documents in arrival order — `finish` validates the count.
    meta: Option<crate::meta::MetaTable>,
    /// Doc-table index at which the current bundle started.
    bundle_start_doc: usize,
    seen: HashSet<String>,
    collapsed: u64,
    target: WriteTarget,
}

/// Where [`finish`](ArchiveWriter::finish) lands the bytes.
enum WriteTarget {
    /// Fresh archive: writes went to `tmp_path`, atomically renamed to `final_path`.
    Create {
        tmp_path: PathBuf,
        final_path: PathBuf,
    },
    /// In-place append (ADR 0010): writes go directly into the existing file, after its old
    /// EOF; the header carries the staged recovery offset until `finish` clears it.
    Append,
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
            compressor: codec::build_compressor()?,
            dict: None,
            meta: None,
            bundle_start_doc: 0,
            seen: HashSet::new(),
            collapsed: 0,
            target: WriteTarget::Create {
                tmp_path,
                final_path,
            },
        })
    }

    /// Open an existing archive for **in-place append** (ADR 0010): the writer is pre-seeded
    /// with the old doc table, bundle table, dictionary, and keys; new documents append after
    /// the old EOF and [`finish`](Self::finish) writes a fresh footer at the new tail. The old
    /// footer stays behind as dead bytes (reclaimed by a re-pack).
    ///
    /// Crash safety: before any byte is written, the old trailer's offset is staged into the
    /// header (bytes `10..16`); every read path falls back to it when the tail is not a valid
    /// trailer, so a crashed or in-progress append leaves the file readable as the pre-append
    /// archive, and the next append overwrites the abandoned tail. `finish` clears the staged
    /// offset last.
    ///
    /// Returns the writer plus the archive's metadata table (rows for the pre-existing
    /// documents); with a schema present the caller must extend it — one row per *stored* new
    /// document — and hand the full table back via [`set_meta_table`](Self::set_meta_table).
    pub fn append<P: AsRef<Path>>(
        path: P,
    ) -> Result<(Self, Option<crate::meta::MetaTable>), ArchiveErr> {
        use std::io::{Read, Seek, SeekFrom};

        let path = path.as_ref();

        // Read the existing footer through the owned-file path (validates header + trailer,
        // with recovery). Only footer-sized regions are copied out — not the document bodies.
        let (docs, bundles, dict, meta, append_start, old_trailer_offset) = {
            let data = fs::read(path).map_err(ArchiveErr::FileRead)?;
            crate::header::validate_header(&data)?;
            let trailer = Trailer::read_from_tail(&data)?;

            let docs = rkyv::from_bytes::<Vec<DocEntry>, Error>(crate::archive::bounded(
                &data,
                trailer.doc_table_offset,
                trailer.doc_table_len,
                "doc table",
            )?)
            .map_err(|e| ArchiveErr::Deserialize(e.to_string()))?;
            let bundles = rkyv::from_bytes::<Vec<BundleDesc>, Error>(crate::archive::bounded(
                &data,
                trailer.bundle_table_offset,
                trailer.bundle_table_len,
                "bundle table",
            )?)
            .map_err(|e| ArchiveErr::Deserialize(e.to_string()))?;
            let dict = if trailer.dict_len > 0 {
                Some(
                    crate::archive::bounded(
                        &data,
                        trailer.dict_offset,
                        trailer.dict_len,
                        "dictionary",
                    )?
                    .to_vec(),
                )
            } else {
                None
            };
            let meta = if trailer.meta_len > 0 {
                Some(
                    rkyv::from_bytes::<crate::meta::MetaTable, Error>(crate::archive::bounded(
                        &data,
                        trailer.meta_offset,
                        trailer.meta_len,
                        "metadata table",
                    )?)
                    .map_err(|e| ArchiveErr::Deserialize(e.to_string()))?,
                )
            } else {
                None
            };

            // The authoritative trailer's position: at the tail normally, or wherever the
            // recovery offset pointed after an abandoned append. New data starts right after
            // it, overwriting any abandoned garbage.
            let trailer_offset = match crate::header::pending_trailer_offset(&data) {
                Some(off) if Trailer::read_at(&data, off as usize).is_ok() => off,
                _ => (data.len() - crate::trailer::TRAILER_LEN) as u64,
            };
            let append_start = trailer_offset + crate::trailer::TRAILER_LEN as u64;
            (docs, bundles, dict, meta, append_start, trailer_offset)
        };

        // Stage the recovery offset, then position the write handle at the append start.
        let mut file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(ArchiveErr::FileWrite)?;
        file.seek(SeekFrom::Start(crate::header::PENDING_TRAILER_AT as u64))
            .map_err(ArchiveErr::FileWrite)?;
        file.write_all(&crate::header::pending_trailer_bytes(old_trailer_offset)?)
            .map_err(ArchiveErr::FileWrite)?;
        file.sync_data().map_err(ArchiveErr::FileWrite)?;
        // Verify the staged offset reads back (belt and braces for the recovery contract).
        {
            let mut hdr = [0u8; HEADER_LEN];
            file.seek(SeekFrom::Start(0))
                .map_err(ArchiveErr::FileWrite)?;
            file.read_exact(&mut hdr).map_err(ArchiveErr::FileRead)?;
            debug_assert_eq!(
                crate::header::pending_trailer_offset(&hdr),
                Some(old_trailer_offset)
            );
        }
        file.seek(SeekFrom::Start(append_start))
            .map_err(ArchiveErr::FileWrite)?;
        file.set_len(append_start).map_err(ArchiveErr::FileWrite)?;

        let compressor = match &dict {
            Some(d) => Compressor::with_dictionary(codec::COMPRESSION_LEVEL, d)
                .map_err(|e| ArchiveErr::Serialize(e.to_string()))?,
            None => codec::build_compressor()?,
        };
        let seen = docs.iter().map(|d| d.key.clone()).collect();
        let bundle_start_doc = docs.len();

        let mut writer = Self {
            out: BufWriter::new(file),
            pos: append_start,
            docs,
            bundles,
            cur_strings: BundleStrings::default(),
            compressor,
            dict,
            meta: None,
            bundle_start_doc,
            seen,
            collapsed: 0,
            target: WriteTarget::Append,
        };
        // The append start sits right after a trailer, which need not be 8-aligned relative
        // to the blobs; align before the first rkyv blob lands.
        writer.pad_to_align()?;
        Ok((writer, meta))
    }

    /// Parse-built push: build the [`HtmlEntry`] (optimal node width + checksum) and append it.
    /// Duplicate keys are skipped (first wins) — the check happens *before* building so
    /// duplicates cost nothing. Returns whether the document was stored (`false` = duplicate).
    pub fn push(&mut self, key: String, html: HtmlDoc) -> Result<bool, ArchiveErr> {
        if self.seen.contains(&key) {
            self.collapsed += 1;
            return Ok(false);
        }
        let entry = HtmlEntry::new(key, html);
        self.push_entry(entry)
    }

    /// Append an already-built entry (used when re-saving an in-memory archive). Same first-wins
    /// dedup as [`push`](Self::push). Consumes the entry so its text pool can be relocated into
    /// the current bundle's string block, leaving the per-document blob string-less.
    pub fn push_entry(&mut self, mut entry: HtmlEntry) -> Result<bool, ArchiveErr> {
        // Dedup before serializing so duplicates cost nothing.
        if !self.seen.insert(entry.key.clone()) {
            self.collapsed += 1;
            return Ok(false);
        }
        // Block boundaries need the node text ranges, so cut before the topology loses its pool.
        let ranges = entry.html.text_ranges();
        let strings = entry.html.take_string_pool();
        let raw_ends = crate::bundle_strings::block_cuts(ranges, strings.len() as u32);
        let bytes =
            rkyv::to_bytes::<Error>(&entry).map_err(|e| ArchiveErr::Serialize(e.to_string()))?;
        self.write_doc(
            &entry.key,
            entry.key_len,
            entry.checksum,
            &bytes,
            &strings,
            &raw_ends,
        )?;
        Ok(true)
    }

    /// Append a document that was already serialized off-thread (the parallel `convert` path
    /// serializes each entry on its worker so the coordinator never holds the live `DomInner`).
    /// Its text pool was relocated into [`SerializedEntry::strings`] **and already compressed
    /// into its block frames** by the worker / two-phase coordinator
    /// ([`SerializedEntry::compress_blocks`], ADR 0005 + ADR 0008), so the writer stores it
    /// verbatim. Same first-wins dedup as [`push_entry`](Self::push_entry).
    pub fn push_serialized(&mut self, entry: &SerializedEntry) -> Result<bool, ArchiveErr> {
        if !self.seen.insert(entry.key.clone()) {
            self.collapsed += 1;
            return Ok(false);
        }
        // A raw pool reaching this point would be stored verbatim as if it were frames and read
        // back as garbage — refuse it loudly instead.
        if entry.frame_ends.len() != entry.raw_ends.len() {
            return Err(ArchiveErr::Serialize(format!(
                "entry '{}' was never compressed (compress_blocks not called)",
                entry.key
            )));
        }
        self.write_doc_blocks(
            &entry.key,
            entry.key_len,
            entry.checksum,
            &entry.bytes,
            &entry.strings,
            &entry.frame_ends,
            &entry.raw_ends,
        )?;
        Ok(true)
    }

    /// The raw-text append path (the in-memory builder's [`push_entry`](Self::push_entry)):
    /// compress the pool's blocks here, dictionary-less, then store them like any frames.
    fn write_doc(
        &mut self,
        key: &str,
        key_len: u16,
        checksum: u64,
        bytes: &[u8],
        strings: &[u8],
        raw_ends: &[u32],
    ) -> Result<(), ArchiveErr> {
        let (frames, frame_ends) =
            codec::compress_pool_blocks(&mut self.compressor, strings, raw_ends)?;
        self.write_doc_blocks(
            key,
            key_len,
            checksum,
            bytes,
            &frames,
            &frame_ends,
            raw_ends,
        )
    }

    /// Append one document's (string-less) blob, record its doc-table row, and accumulate its
    /// already-compressed block frames into the current bundle's string block. The caller has
    /// already performed the dedup `seen` insert. The single source of byte offsets is
    /// `self.pos`.
    #[allow(clippy::too_many_arguments)]
    fn write_doc_blocks(
        &mut self,
        key: &str,
        key_len: u16,
        checksum: u64,
        bytes: &[u8],
        frames: &[u8],
        frame_ends: &[u32],
        raw_ends: &[u32],
    ) -> Result<(), ArchiveErr> {
        let offset = self.pos;
        let len = bytes.len() as u64;
        self.out.write_all(bytes).map_err(ArchiveErr::FileWrite)?;
        self.pos += len;
        self.pad_to_align()?;

        self.cur_strings
            .push_doc_blocks(frames, frame_ends, raw_ends);
        self.docs.push(DocEntry {
            key: key.to_string(),
            key_len,
            checksum,
            offset,
            len,
        });
        Ok(())
    }

    /// Record the archive-wide string dictionary, written into the trailer's dictionary region by
    /// [`finish`](Self::finish). The two-phase convert path calls this once, after training; the
    /// frames it then streams must have been compressed against this same dictionary.
    pub fn set_dictionary(&mut self, dict: Option<Vec<u8>>) {
        self.dict = dict;
    }

    /// Record the typed per-document metadata table (ADR 0009), serialized into the footer by
    /// [`finish`](Self::finish). Row `i` describes the `i`-th *stored* document (arrival order,
    /// after dedup) — callers accumulate rows only for documents that were actually stored.
    pub fn set_meta_table(&mut self, meta: Option<crate::meta::MetaTable>) {
        self.meta = meta;
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

        // The string-compression dictionary region (ADR 0005): the trained dictionary if the
        // two-phase convert path supplied one, else an empty region (length 0) — every frame was
        // then compressed dictionary-less. Either way the bounds stay valid.
        self.pad_to_align()?;
        let dict_offset = self.pos;
        let dict_len = match self.dict.take() {
            Some(dict) => {
                let (off, len) = self.write_blob(&dict)?;
                debug_assert_eq!(off, dict_offset);
                len
            }
            None => 0,
        };

        // The typed metadata table (ADR 0009): rows must align 1:1 with stored documents.
        self.pad_to_align()?;
        let meta_offset = self.pos;
        let meta_len = match self.meta.take() {
            Some(meta) => {
                if meta.row_count() != self.docs.len() {
                    return Err(ArchiveErr::Validate(format!(
                        "metadata table has {} rows, archive has {} documents",
                        meta.row_count(),
                        self.docs.len()
                    )));
                }
                let bytes = rkyv::to_bytes::<Error>(&meta)
                    .map_err(|e| ArchiveErr::Serialize(e.to_string()))?;
                let (off, len) = self.write_blob(&bytes)?;
                debug_assert_eq!(off, meta_offset);
                len
            }
            None => 0,
        };

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
            dict_offset,
            dict_len,
            meta_offset,
            meta_len,
            doc_count: self.docs.len() as u64,
            bundle_count: self.bundles.len() as u64,
        };
        self.out
            .write_all(&trailer.to_bytes())
            .map_err(ArchiveErr::FileWrite)?;
        self.out.flush().map_err(ArchiveErr::FileWrite)?;

        match self.target {
            WriteTarget::Create {
                tmp_path,
                final_path,
            } => {
                drop(self.out);
                fs::rename(&tmp_path, &final_path).map_err(ArchiveErr::FileWrite)?;
            }
            WriteTarget::Append => {
                // Commit order matters (ADR 0010): the new tail must be durable *before* the
                // staged recovery offset is cleared — after this, the tail trailer is
                // authoritative again.
                use std::io::{Seek, SeekFrom};
                let mut file = self
                    .out
                    .into_inner()
                    .map_err(|e| ArchiveErr::FileWrite(std::io::Error::other(e.to_string())))?;
                file.sync_data().map_err(ArchiveErr::FileWrite)?;
                file.seek(SeekFrom::Start(0))
                    .map_err(ArchiveErr::FileWrite)?;
                file.write_all(&header_bytes())
                    .map_err(ArchiveErr::FileWrite)?;
                file.sync_data().map_err(ArchiveErr::FileWrite)?;
            }
        }
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
