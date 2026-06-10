use std::{ops::Index, path::Path};

use crate::{
    builder::HtmlArchiveBuilder,
    bundle::{BUNDLE_CAP, BundleDesc, DocBundle},
    doc_table::{self, DocEntry},
    entry::HtmlEntry,
    error::ArchiveErr,
    trailer::Trailer,
    writer::ArchiveWriter,
};
use fs_err as fs;
use htmlarc_dom::prelude::HtmlDoc;
use rkyv::rancor::Error;

/// An in-memory archive of pre-parsed HTML documents, grouped into [`DocBundle`]s of at most
/// [`BUNDLE_CAP`] documents each (arrival order).
///
/// Bulk iteration (`entries`, `keys`, positional `[i]`) proceeds **bundle→doc**. Keyed lookup
/// ([`get`](Self::get)) instead binary-searches a separate `(key_len, key)`-sorted index, so it
/// stays O(log n) without the bundles having to be globally sorted.
pub struct HtmlArchive {
    bundles: Vec<DocBundle>,
    /// `(bundle, slot)` positions ordered by the referenced entry's `(key_len, key)` — the
    /// keyed-lookup index.
    sorted: Vec<(u32, u32)>,
    /// Cumulative document counts: bundle `b` covers flat positions
    /// `[bundle_starts[b], bundle_starts[b + 1])`. Always has `bundles.len() + 1` entries, so the
    /// last element is the total document count.
    bundle_starts: Vec<usize>,
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
    /// Returns the number of documents written.
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

        // The doc table (bundle→doc order) and bundle table are validated/deserialized up front;
        // every doc blob it points at is then materialized and grouped into its bundle. Each
        // slice is bounds-checked and bytecheck-validated — a corrupt archive becomes an `Err`.
        let doc_slice = bounded(
            &data,
            trailer.doc_table_offset,
            trailer.doc_table_len,
            "doc table",
        )?;
        let docs = rkyv::from_bytes::<Vec<DocEntry>, Error>(doc_slice)
            .map_err(|e| ArchiveErr::Deserialize(e.to_string()))?;

        let bt_slice = bounded(
            &data,
            trailer.bundle_table_offset,
            trailer.bundle_table_len,
            "bundle table",
        )?;
        let bundle_descs = rkyv::from_bytes::<Vec<BundleDesc>, Error>(bt_slice)
            .map_err(|e| ArchiveErr::Deserialize(e.to_string()))?;

        let mut entries = Vec::with_capacity(docs.len());
        for d in &docs {
            let blob = bounded(&data, d.offset, d.len, "document blob")?;
            let entry = rkyv::from_bytes::<HtmlEntry, Error>(blob)
                .map_err(|e| ArchiveErr::Deserialize(e.to_string()))?;
            entries.push(entry);
        }

        // Re-group the flat entry list into bundles per the bundle table.
        let mut entries = entries.into_iter();
        let mut bundles = Vec::with_capacity(bundle_descs.len());
        for desc in &bundle_descs {
            let mut bundle_entries = Vec::with_capacity(desc.doc_count as usize);
            for _ in 0..desc.doc_count {
                let entry = entries.next().ok_or_else(|| {
                    ArchiveErr::Validate("bundle table doc count exceeds the doc table".into())
                })?;
                bundle_entries.push(entry);
            }
            bundles.push(DocBundle::from_entries(bundle_entries));
        }
        if entries.next().is_some() {
            return Err(ArchiveErr::Validate(
                "doc table has more documents than the bundle table accounts for".into(),
            ));
        }

        Ok(Self::from_bundles(bundles))
    }

    /// Iterate every document, **bundle→doc order**.
    pub fn entries(&self) -> impl Iterator<Item = &HtmlEntry> {
        self.bundles.iter().flat_map(|b| b.iter())
    }

    /// Consume the archive, yielding every document in bundle→doc order.
    pub fn into_entries(self) -> impl Iterator<Item = HtmlEntry> {
        self.bundles.into_iter().flat_map(|b| b.into_entries())
    }

    /// The bundles, in order.
    pub fn bundles(&self) -> &[DocBundle] {
        &self.bundles
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries().map(|e| e.key.as_str())
    }

    /// Look up a document by key (binary search over the sorted index).
    pub fn get(&self, key: &str) -> Option<&HtmlEntry> {
        let (b, s) = locate(&self.sorted, &self.bundles, key)?;
        Some(&self.bundles[b][s])
    }

    /// Total document count across all bundles.
    pub fn len(&self) -> usize {
        self.bundle_starts.last().copied().unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Build an archive from a flat document list, grouping it into [`BUNDLE_CAP`]-sized bundles
    /// in order (every bundle but the last is full).
    pub fn from_vec(entries: Vec<HtmlEntry>) -> Self {
        let mut iter = entries.into_iter();
        let mut bundles = Vec::new();
        loop {
            let chunk: Vec<HtmlEntry> = iter.by_ref().take(BUNDLE_CAP).collect();
            if chunk.is_empty() {
                break;
            }
            bundles.push(DocBundle::from_entries(chunk));
        }
        Self::from_bundles(bundles)
    }

    /// Build an archive from pre-formed bundles, computing the sorted index and bundle offsets.
    pub fn from_bundles(bundles: Vec<DocBundle>) -> Self {
        let (sorted, bundle_starts) = index_bundles(&bundles);
        Self {
            bundles,
            sorted,
            bundle_starts,
        }
    }

    /// Map a flat (bundle→doc) position to its `(bundle, slot)`.
    fn locate_flat(&self, i: usize) -> (usize, usize) {
        let b = self.bundle_starts.partition_point(|&start| start <= i) - 1;
        (b, i - self.bundle_starts[b])
    }

    /// Serialize the archive to `path` in the v4 bundle-segmented `.htmlarc` format, streaming
    /// one doc blob at a time (bundle→doc order) through [`ArchiveWriter`]. Each in-memory bundle
    /// is sealed as its own on-disk bundle, so re-saving preserves the existing boundaries (e.g.
    /// the cluster-aligned runs of a ZIM-exported archive) rather than re-chunking at `BUNDLE_CAP`.
    pub fn write_to<P: AsRef<Path>>(&self, path: P) -> Result<(), ArchiveErr> {
        let mut writer = ArchiveWriter::create(path)?;
        for bundle in &self.bundles {
            for entry in bundle.iter() {
                writer.push_entry(entry)?;
            }
            writer.seal_bundle();
        }
        writer.finish()
    }
}

/// Compute the `(key_len, key)`-sorted `(bundle, slot)` index and the cumulative bundle offsets.
fn index_bundles(bundles: &[DocBundle]) -> (Vec<(u32, u32)>, Vec<usize>) {
    let mut bundle_starts = Vec::with_capacity(bundles.len() + 1);
    let mut total = 0usize;
    bundle_starts.push(0);
    for b in bundles {
        total += b.len();
        bundle_starts.push(total);
    }

    let mut sorted: Vec<(u32, u32)> = Vec::with_capacity(total);
    for (bi, b) in bundles.iter().enumerate() {
        for slot in 0..b.len() {
            sorted.push((bi as u32, slot as u32));
        }
    }
    sorted.sort_by(|&(ba, sa), &(bb, sb)| {
        let a = &bundles[ba as usize][sa as usize];
        let c = &bundles[bb as usize][sb as usize];
        doc_table::compare(a.key_len, &a.key, c.key_len, &c.key)
    });

    (sorted, bundle_starts)
}

/// Binary-search the sorted `(bundle, slot)` index for `key`. A free function (not a method) so
/// the comparator borrows the `bundles` slice rather than `self`, leaving `sorted` free to search.
fn locate(sorted: &[(u32, u32)], bundles: &[DocBundle], key: &str) -> Option<(usize, usize)> {
    let key_len = doc_table::grapheme_len(key);
    let pos = sorted
        .binary_search_by(|&(b, s)| {
            let e = &bundles[b as usize][s as usize];
            doc_table::compare(e.key_len, &e.key, key_len, key)
        })
        .ok()?;
    let (b, s) = sorted[pos];
    Some((b as usize, s as usize))
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
        let (b, s) = self.locate_flat(index);
        &self.bundles[b][s]
    }
}

impl crate::Archive for HtmlArchive {
    type Entry = HtmlEntry;

    fn len(&self) -> usize {
        HtmlArchive::len(self)
    }

    fn get(&self, key: &str) -> Result<Option<&HtmlEntry>, ArchiveErr> {
        // Owned entries were all validated at `read_from`, so lookup is infallible.
        Ok(HtmlArchive::get(self, key))
    }

    fn entries(&self) -> impl Iterator<Item = &HtmlEntry> {
        HtmlArchive::entries(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use htmlarc_dom::prelude::HtmlDoc;

    fn entry(key: &str) -> HtmlEntry {
        HtmlEntry::new(key.to_string(), HtmlDoc::parse("<p>x</p>").unwrap())
    }

    /// `locate_flat` must map every flat position to the right `(bundle, slot)` across bundle
    /// boundaries — the load-bearing invariant for positional access over `Vec<DocBundle>`.
    #[test]
    fn locate_flat_boundaries() {
        // Two full bundles + a partial third (exercises 0, last-of-bundle, first-of-bundle, end).
        let n = BUNDLE_CAP * 2 + 3;
        let entries: Vec<HtmlEntry> = (0..n).map(|i| entry(&format!("k{i:08}"))).collect();
        let archive = HtmlArchive::from_vec(entries);

        assert_eq!(archive.len(), n);
        assert_eq!(archive.bundles().len(), 3);

        for &(i, eb, es) in &[
            (0usize, 0usize, 0usize),
            (BUNDLE_CAP - 1, 0, BUNDLE_CAP - 1),
            (BUNDLE_CAP, 1, 0),
            (BUNDLE_CAP + 1, 1, 1),
            (BUNDLE_CAP * 2, 2, 0),
            (n - 1, 2, 2),
        ] {
            assert_eq!(archive.locate_flat(i), (eb, es), "flat index {i}");
        }
    }
}
