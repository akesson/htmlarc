use std::{ops::Index, path::Path};

use crate::{
    builder::HtmlArchiveBuilder, directory::DirEntry, entry::HtmlEntry, error::ArchiveErr,
    trailer::Trailer, writer::ArchiveWriter,
};
use fs_err as fs;
use htmlarc_dom::prelude::HtmlDoc;
use rkyv::rancor::Error;
use unicode_segmentation::UnicodeSegmentation;

pub struct HtmlArchive {
    pub entries: Vec<HtmlEntry>,
}

impl HtmlArchive {
    /// Open a *source* as an archive. The source may be:
    /// - a directory: every `*.html`/`*.htm` file is parsed and keyed by file stem;
    /// - a single `.html`/`.htm` file: parsed into a one-entry archive;
    /// - anything else (e.g. a `.htmlarc` file): loaded via [`Self::read_from`].
    ///
    /// Note the performance model: a `.htmlarc` is a cheap rkyv load, whereas html
    /// files/directories are re-parsed on every call. Pack once for repeated querying.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, ArchiveErr> {
        let path = path.as_ref();
        if path.is_dir() {
            Self::from_html_dir(path)
        } else if is_html_path(path) {
            let mut builder = HtmlArchiveBuilder::default();
            add_html_file(&mut builder, path)?;
            Ok(builder.build())
        } else {
            Self::read_from(path)
        }
    }

    /// Stream a *source* straight to a `.htmlarc` at `output`, holding at most one parsed
    /// document in memory at a time (the streaming pack path). The source may be:
    /// - a directory: every `*.html`/`*.htm` file is parsed, appended, and dropped;
    /// - a single `.html`/`.htm` file;
    /// - an existing `.htmlarc`: loaded and re-saved (loading is inherent to re-saving).
    ///
    /// Returns the number of documents written. Unlike `open(..).write_to(..)`, a directory of
    /// HTML never holds the whole corpus resident.
    pub fn pack_to<P: AsRef<Path>, Q: AsRef<Path>>(
        source: P,
        output: Q,
    ) -> Result<usize, ArchiveErr> {
        let source = source.as_ref();
        if source.is_dir() {
            let mut paths = fs::read_dir(source)
                .map_err(ArchiveErr::FileRead)?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| is_html_path(p))
                .collect::<Vec<_>>();
            paths.sort();
            let mut writer = ArchiveWriter::create(output)?;
            for p in &paths {
                stream_html_file(&mut writer, p)?;
            }
            let n = writer.doc_count();
            writer.finish()?;
            Ok(n)
        } else if is_html_path(source) {
            let mut writer = ArchiveWriter::create(output)?;
            stream_html_file(&mut writer, source)?;
            let n = writer.doc_count();
            writer.finish()?;
            Ok(n)
        } else {
            let archive = Self::read_from(source)?;
            let n = archive.len();
            archive.write_to(output)?;
            Ok(n)
        }
    }

    /// Build an archive by parsing every `*.html`/`*.htm` file in `dir` (non-recursive).
    pub fn from_html_dir<P: AsRef<Path>>(dir: P) -> Result<Self, ArchiveErr> {
        let mut builder = HtmlArchiveBuilder::default();
        let mut paths = fs::read_dir(dir.as_ref())
            .map_err(ArchiveErr::FileRead)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| is_html_path(p))
            .collect::<Vec<_>>();
        paths.sort();
        for p in &paths {
            add_html_file(&mut builder, p)?;
        }
        Ok(builder.build())
    }

    pub fn read_from<P: AsRef<Path>>(path: P) -> Result<Self, ArchiveErr> {
        let data = fs::read(path).map_err(ArchiveErr::FileRead)?;
        crate::header::validate_header(&data)?;
        let trailer = Trailer::read_from_tail(&data)?;

        // Read the footer directory, then materialize every doc blob it points at. The
        // directory is sorted by (key_len, key), so the resulting Vec is already sorted for
        // binary search. Each slice is bounds-checked and validated (bytecheck) — a corrupt
        // archive becomes an `Err`, never UB.
        let dir_slice = bounded(&data, trailer.dir_offset, trailer.dir_len, "directory")?;
        let dir = rkyv::from_bytes::<Vec<DirEntry>, Error>(dir_slice)
            .map_err(|e| ArchiveErr::Deserialize(e.to_string()))?;

        let mut entries = Vec::with_capacity(dir.len());
        for d in &dir {
            let blob = bounded(&data, d.offset, d.len, "document blob")?;
            let entry = rkyv::from_bytes::<HtmlEntry, Error>(blob)
                .map_err(|e| ArchiveErr::Deserialize(e.to_string()))?;
            entries.push(entry);
        }
        Ok(Self { entries })
    }

    pub fn entries(&self) -> impl Iterator<Item = &HtmlEntry> {
        self.entries.iter()
    }

    pub fn into_entries(self) -> impl Iterator<Item = HtmlEntry> {
        self.entries.into_iter()
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| e.key.as_str())
    }

    pub fn get(&self, key: &str) -> Option<&HtmlEntry> {
        let key_len = key.graphemes(true).count() as u16;
        let found = self
            .entries
            .binary_search_by(|e| {
                e.key_len
                    .cmp(&key_len)
                    .then_with(|| e.key.as_str().cmp(key))
            })
            .ok()?;

        Some(&self.entries[found])
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn from_vec(entries: Vec<HtmlEntry>) -> Self {
        HtmlArchive { entries }
    }

    /// Serialize the archive to `path` in the v3 `.htmlarc` footer-indexed format, streaming
    /// one doc blob at a time through [`ArchiveWriter`].
    pub fn write_to<P: AsRef<Path>>(&self, path: P) -> Result<(), ArchiveErr> {
        let mut writer = ArchiveWriter::create(path)?;
        for entry in &self.entries {
            writer.push_entry(entry)?;
        }
        writer.finish()
    }
}

/// Slice `data[offset..offset+len]`, returning a validation error (not a panic) if the range
/// falls outside the file — guards against a corrupt directory/trailer.
fn bounded<'a>(data: &'a [u8], offset: u64, len: u64, what: &str) -> Result<&'a [u8], ArchiveErr> {
    let end = offset
        .checked_add(len)
        .ok_or_else(|| ArchiveErr::Validate(format!("{what} offset/len overflow")))?;
    data.get(offset as usize..end as usize)
        .ok_or_else(|| ArchiveErr::Validate(format!("{what} range lies outside the file")))
}

fn is_html_path(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("html") | Some("htm")
    )
}

fn add_html_file(builder: &mut HtmlArchiveBuilder, path: &Path) -> Result<(), ArchiveErr> {
    let contents = fs::read_to_string(path).map_err(ArchiveErr::FileRead)?;
    let doc = HtmlDoc::parse(&contents)
        .map_err(|e| ArchiveErr::Parse(format!("{}: {e}", path.display())))?;
    let key = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();
    builder.add_html(key, doc);
    Ok(())
}

/// Parse one HTML file and stream it straight into the writer (dropping it immediately).
fn stream_html_file(writer: &mut ArchiveWriter, path: &Path) -> Result<(), ArchiveErr> {
    let contents = fs::read_to_string(path).map_err(ArchiveErr::FileRead)?;
    let doc = HtmlDoc::parse(&contents)
        .map_err(|e| ArchiveErr::Parse(format!("{}: {e}", path.display())))?;
    let key = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();
    writer.push(key, doc)
}

impl Index<usize> for HtmlArchive {
    type Output = HtmlEntry;

    fn index(&self, index: usize) -> &Self::Output {
        &self.entries[index]
    }
}

impl crate::Archive for HtmlArchive {
    type Entry = HtmlEntry;

    fn len(&self) -> usize {
        self.entries.len()
    }

    fn get(&self, key: &str) -> Result<Option<&HtmlEntry>, ArchiveErr> {
        // Owned entries were all validated at `read_from`, so lookup is infallible.
        Ok(HtmlArchive::get(self, key))
    }

    fn entries(&self) -> impl Iterator<Item = &HtmlEntry> {
        self.entries.iter()
    }
}
