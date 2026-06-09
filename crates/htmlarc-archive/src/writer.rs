//! The streaming archive writer.
//!
//! Where the old format buffered the whole `Vec<HtmlEntry>` and serialized it in one shot,
//! [`ArchiveWriter`] serializes and appends each document the moment it's pushed, then drops
//! it. Only a small in-RAM directory (one [`DirEntry`] per doc) plus a dedup key-set survive
//! across the pack, so peak RSS no longer scales with the whole corpus.
//!
//! On-disk result: `[header][doc blob][doc blob]… [dict region][directory blob][trailer]`,
//! every blob/region 8-byte aligned (rkyv with `pointer_width_64` needs 8-aligned starts).
//! The file is written to a sibling temp path and atomically renamed on [`finish`](ArchiveWriter::finish),
//! so an open `MmapArchive` over a previous build is never corrupted in place.

use std::collections::HashSet;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use fs_err as fs;
use fs_err::File;
use htmlarc_dom::prelude::HtmlDoc;
use rkyv::rancor::Error;

use crate::directory::DirEntry;
use crate::entry::HtmlEntry;
use crate::error::ArchiveErr;
use crate::header::{HEADER_LEN, header_bytes};
use crate::trailer::Trailer;

const ALIGN: u64 = 8;

/// Streams documents into a v3 `.htmlarc`, one blob at a time.
pub struct ArchiveWriter {
    out: BufWriter<File>,
    /// Running byte position in the file (header + blobs + padding). The single source of
    /// truth for every recorded offset — never derived from the `BufWriter`'s own position.
    pos: u64,
    dir: Vec<DirEntry>,
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
            dir: Vec::new(),
            seen: HashSet::new(),
            collapsed: 0,
            tmp_path,
            final_path,
        })
    }

    /// Parse-built push: build the [`HtmlEntry`] (optimal node width + checksum) and append it.
    /// Duplicate keys are skipped (first wins), matching the old `BTreeSet` behavior — the
    /// check happens *before* building so duplicates cost nothing.
    pub fn push(&mut self, key: String, html: HtmlDoc) -> Result<(), ArchiveErr> {
        if self.seen.contains(&key) {
            self.collapsed += 1;
            return Ok(());
        }
        let entry = HtmlEntry::new(key, html);
        self.push_entry(&entry)
    }

    /// Append an already-built entry (used when re-saving an in-memory archive). Same first-wins
    /// dedup as [`push`](Self::push).
    pub(crate) fn push_entry(&mut self, entry: &HtmlEntry) -> Result<(), ArchiveErr> {
        if !self.seen.insert(entry.key.clone()) {
            self.collapsed += 1;
            return Ok(());
        }
        let bytes =
            rkyv::to_bytes::<Error>(entry).map_err(|e| ArchiveErr::Serialize(e.to_string()))?;
        let offset = self.pos;
        let len = bytes.len() as u64;
        self.out.write_all(&bytes).map_err(ArchiveErr::FileWrite)?;
        self.pos += len;
        self.pad_to_align()?;

        self.dir.push(DirEntry {
            key: entry.key.clone(),
            key_len: entry.key_len,
            checksum: entry.checksum,
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

    /// Finalize: write the (empty) dict region, the sorted directory, and the trailer, then
    /// flush and atomically rename the temp file onto the target path.
    pub fn finish(mut self) -> Result<(), ArchiveErr> {
        // Shared-dict region: empty for now, but keep its start 8-aligned and recorded so a
        // future compression piece can populate it without another format bump.
        self.pad_to_align()?;
        let dict_offset = self.pos;
        let dict_len = 0u64;

        // Directory: sorted by (key_len, key) so readers binary-search it.
        self.pad_to_align()?;
        self.dir
            .sort_by(|a, b| crate::directory::compare(a.key_len, &a.key, b.key_len, &b.key));
        let dir_offset = self.pos;
        let dir_bytes =
            rkyv::to_bytes::<Error>(&self.dir).map_err(|e| ArchiveErr::Serialize(e.to_string()))?;
        let dir_len = dir_bytes.len() as u64;
        self.out
            .write_all(&dir_bytes)
            .map_err(ArchiveErr::FileWrite)?;
        self.pos += dir_len;

        let trailer = Trailer {
            dir_offset,
            dir_len,
            dict_offset,
            dict_len,
            doc_count: self.dir.len() as u64,
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
        self.dir.len()
    }
}
