//! WARC source: web crawl archives (e.g. Common Crawl).
//!
//! A minimal WARC/1.0 reader: it walks `response` records, extracts the `text/html` HTTP
//! bodies, and keys them by `WARC-Target-URI`. `.warc.gz` files (one gzip member per record,
//! the Common Crawl convention, but concatenated members and single-stream files are handled
//! transparently) are read through a single-member [`GzDecoder`]; plain `.warc` files are read
//! directly.
//!
//! **Lazy, bounded memory.** Pass 1 ([`WarcSource::open`]) scans every file once to count,
//! dedup, and collect a per-document *locator* — the file, the compressed offset of its gzip
//! member, and its ordinal within that member — but **discards the bodies**. The locators are
//! chunked into bundle-sized runs. [`drive_run`](WarcSource::drive_run) then re-reads only the
//! bodies of one run at a time by seeking to each member, so peak memory is bounded by a run,
//! not by the corpus. (This is the same lazy shape ZIM and directory sources already use.)

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use flate2::bufread::GzDecoder;
use htmlarc_archive::BUNDLE_CAP;

use super::{DocSink, Source, SourceStats};

/// Reject a record claiming a wildly oversized body rather than trying to allocate it — a
/// corrupt length would otherwise abort the process.
const MAX_BLOCK: usize = 256 << 20;

/// How to re-read one document's body in pass 2 without having kept it from pass 1.
///
/// `member_offset` is the byte offset at which to start reading in the (possibly compressed)
/// file; for `.gz` it is the start of the gzip member, for plain `.warc` the start of the
/// record. `in_member_ord` is the record's index within the stream decoded from that offset
/// (0 for the Common Crawl one-record-per-member layout and for plain `.warc`; non-zero only
/// when several records share one gzip member).
#[derive(Clone)]
struct Loc {
    file_idx: u32,
    member_offset: u64,
    in_member_ord: u32,
    key: String,
}

pub(crate) struct WarcSource {
    /// The WARC files, in sorted order; `Loc::file_idx` indexes this.
    files: Vec<PathBuf>,
    /// Bundle-sized runs of locators, in pass-1 order (so bundles are deterministic and
    /// identical to the eager reader's).
    runs: Vec<Vec<Loc>>,
    stats: SourceStats,
}

fn has_suffix(path: &Path, suffix: &str) -> bool {
    path.file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase().ends_with(suffix))
        .unwrap_or(false)
}

pub(crate) fn gather_warc_files(input: &Path) -> Result<Vec<PathBuf>> {
    if input.is_dir() {
        let mut files: Vec<PathBuf> = std::fs::read_dir(input)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| has_suffix(p, ".warc") || has_suffix(p, ".warc.gz"))
            .collect();
        files.sort();
        if files.is_empty() {
            bail!("no .warc/.warc.gz files in {}", input.display());
        }
        Ok(files)
    } else {
        Ok(vec![input.to_path_buf()])
    }
}

/// One parsed WARC record (only the fields this reader needs).
struct Record {
    warc_type: String,
    target_uri: String,
    block: Vec<u8>,
}

/// Read one trimmed line (without the trailing CR/LF) into `buf`; `Ok(false)` at EOF.
fn read_line<R: BufRead>(r: &mut R, buf: &mut Vec<u8>) -> Result<bool> {
    buf.clear();
    if r.read_until(b'\n', buf)? == 0 {
        return Ok(false);
    }
    while matches!(buf.last(), Some(b'\r') | Some(b'\n')) {
        buf.pop();
    }
    Ok(true)
}

/// Read the next WARC record, or `None` at end of stream. The blank line(s) that separate
/// records are consumed by the version-line search at the top.
fn read_record<R: BufRead>(r: &mut R) -> Result<Option<Record>> {
    let mut line = Vec::new();
    loop {
        if !read_line(r, &mut line)? {
            return Ok(None);
        }
        if !line.is_empty() {
            break;
        }
    }
    if !line.starts_with(b"WARC/") {
        bail!(
            "expected a WARC record, found {:?}",
            String::from_utf8_lossy(&line)
        );
    }

    let mut content_length = 0usize;
    let mut warc_type = String::new();
    let mut target_uri = String::new();
    loop {
        if !read_line(r, &mut line)? || line.is_empty() {
            break; // blank line ends the header block
        }
        let Some(colon) = line.iter().position(|&b| b == b':') else {
            continue;
        };
        let name = String::from_utf8_lossy(&line[..colon])
            .trim()
            .to_ascii_lowercase();
        let value = String::from_utf8_lossy(&line[colon + 1..])
            .trim()
            .to_string();
        match name.as_str() {
            "content-length" => content_length = value.parse().unwrap_or(0),
            "warc-type" => warc_type = value.to_ascii_lowercase(),
            "warc-target-uri" => target_uri = value,
            _ => {}
        }
    }

    if content_length > MAX_BLOCK {
        bail!("WARC record claims an implausible Content-Length of {content_length} bytes");
    }
    let mut block = vec![0u8; content_length];
    r.read_exact(&mut block)?;
    Ok(Some(Record {
        warc_type,
        target_uri,
        block,
    }))
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Extract the HTML body of a `response` record, or `None` if it is not an HTML response.
fn html_body(rec: &Record) -> Option<&[u8]> {
    if rec.warc_type != "response" {
        return None;
    }
    let sep = find(&rec.block, b"\r\n\r\n")?;
    let headers = String::from_utf8_lossy(&rec.block[..sep]).to_ascii_lowercase();
    let is_html = headers
        .lines()
        .any(|l| l.starts_with("content-type:") && l.contains("text/html"));
    is_html.then(|| &rec.block[sep + 4..])
}

/// Walks a WARC file's records in order, tagging each with a re-read locator
/// (`member_offset`, `in_member_ord`). For `.gz` it iterates gzip members one at a time
/// (capturing each member's compressed offset via the exact-consumption guarantee that also
/// lets `MultiGzDecoder` chain members); for plain `.warc` it reads sequentially, tagging each
/// record with its byte offset.
struct RecordWalker {
    reader: BufReader<File>,
    gz: bool,
    /// The currently-decoded gzip member being drained (gz only).
    member: Option<MemberCursor>,
}

struct MemberCursor {
    offset: u64,
    cursor: Cursor<Vec<u8>>,
    next_ord: u32,
}

impl RecordWalker {
    fn open(path: &Path, gz: bool) -> Result<Self> {
        let f =
            File::open(path).map_err(|e| anyhow!("could not open WARC {}: {e}", path.display()))?;
        Ok(Self {
            reader: BufReader::new(f),
            gz,
            member: None,
        })
    }

    /// The next record with its locator coordinates, or `None` at end of file. A truncated or
    /// corrupt tail also ends the walk (after a warning), matching the converter's
    /// skip-and-continue policy for unreadable input.
    fn next_record(&mut self) -> Option<(u64, u32, Record)> {
        if self.gz {
            self.next_gz()
        } else {
            self.next_plain()
        }
    }

    fn next_plain(&mut self) -> Option<(u64, u32, Record)> {
        let offset = self.reader.stream_position().ok()?;
        match read_record(&mut self.reader) {
            Ok(Some(rec)) => Some((offset, 0, rec)),
            Ok(None) => None,
            Err(e) => {
                eprintln!("warc: stopping at an unreadable record ({e})");
                None
            }
        }
    }

    fn next_gz(&mut self) -> Option<(u64, u32, Record)> {
        loop {
            if self.member.is_none() {
                let offset = self.reader.stream_position().ok()?;
                match self.reader.fill_buf() {
                    Ok([]) => return None, // clean EOF at a member boundary
                    Ok(_) => {}
                    Err(e) => {
                        eprintln!("warc: stopping at an unreadable member ({e})");
                        return None;
                    }
                }
                let mut buf = Vec::new();
                {
                    // A single-member decoder consumes exactly this member's bytes, leaving
                    // `reader` positioned at the next member's start.
                    let mut dec = GzDecoder::new(&mut self.reader);
                    if let Err(e) = dec.read_to_end(&mut buf) {
                        eprintln!("warc: stopping at an unreadable gzip member ({e})");
                        return None;
                    }
                }
                self.member = Some(MemberCursor {
                    offset,
                    cursor: Cursor::new(buf),
                    next_ord: 0,
                });
            }

            let m = self.member.as_mut().unwrap();
            match read_record(&mut m.cursor) {
                Ok(Some(rec)) => {
                    let ord = m.next_ord;
                    m.next_ord += 1;
                    return Some((m.offset, ord, rec));
                }
                Ok(None) => {}
                Err(e) => {
                    eprintln!("warc: stopping at an unreadable record ({e})");
                    return None;
                }
            }
            // This member is drained — advance to the next one.
            self.member = None;
        }
    }
}

impl WarcSource {
    pub(crate) fn open(
        input: &Path,
        wordlist: Option<&HashSet<String>>,
        limit: Option<usize>,
    ) -> Result<Self> {
        let files = gather_warc_files(input)?;
        let mut stats = SourceStats::default();
        let mut seen: HashSet<String> = HashSet::new();
        let mut locs: Vec<Loc> = Vec::new();

        // Pass 1: scan every file once, recording a locator per selected document and
        // discarding the bodies. Selection order/filtering is identical to the eager reader,
        // so the resulting bundles are byte-for-byte the same.
        'files: for (idx, path) in files.iter().enumerate() {
            let file_idx = idx as u32;
            let gz = has_suffix(path, ".gz");
            let mut walker = RecordWalker::open(path, gz)?;
            while let Some((member_offset, in_member_ord, rec)) = walker.next_record() {
                if let Some(lim) = limit
                    && locs.len() >= lim
                {
                    break 'files;
                }
                if html_body(&rec).is_none() {
                    continue;
                }
                if rec.target_uri.is_empty() {
                    continue;
                }
                if let Some(set) = wordlist
                    && !set.contains(&rec.target_uri)
                {
                    stats.ignored += 1;
                    continue;
                }
                if !seen.insert(rec.target_uri.clone()) {
                    stats.collapsed += 1;
                    continue;
                }
                locs.push(Loc {
                    file_idx,
                    member_offset,
                    in_member_ord,
                    key: rec.target_uri,
                });
            }
        }

        stats.prepared = locs.len();
        let runs = locs
            .chunks(BUNDLE_CAP)
            .map(|c| c.to_vec())
            .collect::<Vec<_>>();
        Ok(Self { files, runs, stats })
    }
}

/// Read the body for one locator: seek to its member, decode just that member (for `.gz`),
/// skip to the recorded ordinal, and return the HTML body. `Ok(None)` if the record can no
/// longer be read as HTML at the recorded position.
fn read_body(file: &mut File, gz: bool, loc: &Loc) -> Result<Option<String>> {
    file.seek(SeekFrom::Start(loc.member_offset))?;
    if gz {
        let mut buf = Vec::new();
        {
            let mut dec = GzDecoder::new(BufReader::new(&mut *file));
            dec.read_to_end(&mut buf)?;
        }
        read_nth_body(&mut Cursor::new(buf), loc.in_member_ord)
    } else {
        read_nth_body(&mut BufReader::new(&mut *file), loc.in_member_ord)
    }
}

/// Skip `ord` records, then return the next record's HTML body (or `None` if it ran out or the
/// record is not an HTML response).
fn read_nth_body<R: BufRead>(r: &mut R, ord: u32) -> Result<Option<String>> {
    for _ in 0..ord {
        if read_record(r)?.is_none() {
            return Ok(None);
        }
    }
    Ok(match read_record(r)? {
        Some(rec) => html_body(&rec).map(|b| String::from_utf8_lossy(b).into_owned()),
        None => None,
    })
}

impl Source for WarcSource {
    fn run_count(&self) -> usize {
        self.runs.len()
    }

    fn drive_run(&self, rank: usize, sink: &mut dyn DocSink) -> u32 {
        let mut read_failed = 0u32;
        let run = &self.runs[rank];
        let mut i = 0;
        // Locators within a run are in pass-1 order, so consecutive ones share a file: open
        // each file once and re-read its members in turn.
        while i < run.len() {
            let file_idx = run[i].file_idx;
            let path = &self.files[file_idx as usize];
            let gz = has_suffix(path, ".gz");
            match File::open(path) {
                Ok(mut file) => {
                    while i < run.len() && run[i].file_idx == file_idx {
                        let loc = &run[i];
                        match read_body(&mut file, gz, loc) {
                            Ok(Some(body)) => sink.accept(&loc.key, &body),
                            Ok(None) => {
                                eprintln!(
                                    "warc: '{}' no longer readable at its recorded offset",
                                    loc.key
                                );
                                read_failed += 1;
                            }
                            Err(e) => {
                                eprintln!("warc: re-read failed for '{}': {e}", loc.key);
                                read_failed += 1;
                            }
                        }
                        i += 1;
                    }
                }
                Err(e) => {
                    eprintln!("could not reopen WARC {}: {e}", path.display());
                    while i < run.len() && run[i].file_idx == file_idx {
                        read_failed += 1;
                        i += 1;
                    }
                }
            }
        }
        read_failed
    }

    fn stats(&self) -> &SourceStats {
        &self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::Write;

    struct Collect(Vec<(String, String)>);
    impl DocSink for Collect {
        fn accept(&mut self, key: &str, html: &str) {
            self.0.push((key.to_string(), html.to_string()));
        }
    }

    /// A minimal WARC `response` record (HTTP headers + body), trailing CRLFs as separators.
    fn record(uri: &str, body: &str) -> Vec<u8> {
        let http = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n{body}");
        format!(
            "WARC/1.0\r\nWARC-Type: response\r\nWARC-Target-URI: {uri}\r\nContent-Length: {}\r\n\r\n{http}\r\n\r\n",
            http.len()
        )
        .into_bytes()
    }

    fn gz_member(data: &[u8]) -> Vec<u8> {
        let mut e = GzEncoder::new(Vec::new(), Compression::fast());
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    fn write_tmp(name: &str, bytes: &[u8]) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("htmlarc-warc-test-{}-{name}", std::process::id()));
        std::fs::write(&p, bytes).unwrap();
        p
    }

    fn roundtrip(path: &Path) -> Vec<(String, String)> {
        let src = WarcSource::open(path, None, None).unwrap();
        let mut got = Vec::new();
        for rank in 0..src.run_count() {
            let mut c = Collect(Vec::new());
            src.drive_run(rank, &mut c);
            got.extend(c.0);
        }
        got
    }

    fn want(docs: &[(&str, &str)]) -> Vec<(String, String)> {
        docs.iter()
            .map(|(u, b)| (u.to_string(), b.to_string()))
            .collect()
    }

    /// Common Crawl layout: every record is its own concatenated gzip member. This is the
    /// case the lazy reader's per-member offset capture must get exactly right.
    #[test]
    fn gz_one_member_per_record_roundtrips() {
        let docs = [
            ("http://a/", "<html><body>A</body></html>"),
            ("http://b/", "<html><body>BB</body></html>"),
            ("http://c/", "<html><body>CCC</body></html>"),
        ];
        let mut file = Vec::new();
        for (uri, body) in &docs {
            file.extend_from_slice(&gz_member(&record(uri, body)));
        }
        let path = write_tmp("members.warc.gz", &file);
        let got = roundtrip(&path);
        std::fs::remove_file(&path).ok();
        assert_eq!(got, want(&docs));
    }

    /// Non-CC layout: all records in a single gzip member. Exercises `in_member_ord` skipping.
    #[test]
    fn gz_single_stream_many_records_roundtrips() {
        let docs = [
            ("http://x/", "<html>1</html>"),
            ("http://y/", "<html>22</html>"),
            ("http://z/", "<html>333</html>"),
        ];
        let mut all = Vec::new();
        for (uri, body) in &docs {
            all.extend_from_slice(&record(uri, body));
        }
        let path = write_tmp("single.warc.gz", &gz_member(&all));
        let got = roundtrip(&path);
        std::fs::remove_file(&path).ok();
        assert_eq!(got, want(&docs));
    }

    /// Plain (uncompressed) `.warc`: locator is a byte offset.
    #[test]
    fn plain_warc_roundtrips() {
        let docs = [
            ("http://p/", "<html>p</html>"),
            ("http://q/", "<html>qq</html>"),
        ];
        let mut all = Vec::new();
        for (uri, body) in &docs {
            all.extend_from_slice(&record(uri, body));
        }
        let path = write_tmp("plain.warc", &all);
        let got = roundtrip(&path);
        std::fs::remove_file(&path).ok();
        assert_eq!(got, want(&docs));
    }

    /// Non-html responses and a duplicate key are filtered in pass 1, exactly as before.
    #[test]
    fn skips_non_html_and_dedups() {
        let html = record("http://dup/", "<html>first</html>");
        let dup = record("http://dup/", "<html>second</html>");
        let css = {
            let http = "HTTP/1.1 200 OK\r\nContent-Type: text/css\r\n\r\nbody{}";
            format!(
                "WARC/1.0\r\nWARC-Type: response\r\nWARC-Target-URI: http://css/\r\nContent-Length: {}\r\n\r\n{http}\r\n\r\n",
                http.len()
            )
            .into_bytes()
        };
        let mut file = Vec::new();
        for rec in [&html, &css, &dup] {
            file.extend_from_slice(&gz_member(rec));
        }
        let path = write_tmp("filtered.warc.gz", &file);
        let src = WarcSource::open(&path, None, None).unwrap();
        let got = roundtrip(&path);
        std::fs::remove_file(&path).ok();
        // Only the first http://dup/ survives; the css is not counted, the dup is collapsed.
        assert_eq!(got, want(&[("http://dup/", "<html>first</html>")]));
        assert_eq!(src.stats().prepared, 1);
        assert_eq!(src.stats().collapsed, 1);
    }
}
