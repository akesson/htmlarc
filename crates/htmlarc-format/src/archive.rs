use std::{ops::Index, path::Path};

use crate::{builder::HtmlArchiveBuilder, entry::HtmlEntry, error::ArchiveErr};
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
        let offset = crate::header::payload_offset(&data)?;
        // Safe, validated deserialize (bytecheck) — rejects a corrupt archive with an
        // `Err` instead of the unsoundness of the previous unchecked load. Handles
        // both modern (header-prefixed) and legacy (header-less) files.
        let entries = rkyv::from_bytes::<Vec<HtmlEntry>, Error>(&data[offset..])
            .map_err(|e| ArchiveErr::Deserialize(e.to_string()))?;
        Ok(Self { entries })
    }

    pub fn entries(&self) -> impl Iterator<Item = &HtmlEntry> {
        self.entries.iter()
    }

    pub fn into_entries(self) -> impl Iterator<Item = HtmlEntry> {
        self.entries.into_iter()
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.entries.iter().map(|e| &e.key)
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

    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn from_vec(entries: Vec<HtmlEntry>) -> Self {
        HtmlArchive { entries }
    }

    /// Serialize the archive to `path` (the `.htmlarc` binary format): a 16-byte
    /// header followed by the rkyv payload.
    pub fn write_to<P: AsRef<Path>>(&self, path: P) -> Result<(), ArchiveErr> {
        let data = rkyv::to_bytes::<Error>(&self.entries)
            .map_err(|e| ArchiveErr::Serialize(e.to_string()))?;
        let header = crate::header::header_bytes();
        let mut out = Vec::with_capacity(header.len() + data.len());
        out.extend_from_slice(&header);
        out.extend_from_slice(&data);
        fs::write(path, out).map_err(ArchiveErr::FileWrite)?;
        Ok(())
    }
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

impl Index<usize> for HtmlArchive {
    type Output = HtmlEntry;

    fn index(&self, index: usize) -> &Self::Output {
        &self.entries[index]
    }
}
