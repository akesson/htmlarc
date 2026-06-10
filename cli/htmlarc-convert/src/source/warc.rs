//! WARC source: web crawl archives (e.g. Common Crawl).
//!
//! A minimal WARC/1.0 reader: it walks `response` records, extracts the `text/html` HTTP
//! bodies, and keys them by `WARC-Target-URI`. `.warc.gz` files (one gzip member per record,
//! the Common Crawl convention, but concatenated members are handled transparently) are read
//! through a [`MultiGzDecoder`]; plain `.warc` files are read directly.
//!
//! Pass 1 reads the whole input into bundle-sized runs of owned `(key, html)` pairs. This
//! holds all selected HTML in memory, so for very large crawls use `--limit` to sample; the
//! `stats` probe — the primary WARC consumer — is normally run on a sample anyway. (A future
//! step can index record offsets to read lazily.)

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use flate2::bufread::MultiGzDecoder;
use htmlarc_archive::BUNDLE_CAP;

use super::{DocSink, Source, SourceStats};

/// Reject a record claiming a wildly oversized body rather than trying to allocate it — a
/// corrupt length would otherwise abort the process.
const MAX_BLOCK: usize = 256 << 20;

pub(crate) struct WarcSource {
    runs: Vec<Vec<(String, String)>>,
    stats: SourceStats,
}

fn has_suffix(path: &Path, suffix: &str) -> bool {
    path.file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase().ends_with(suffix))
        .unwrap_or(false)
}

fn gather_warc_files(input: &Path) -> Result<Vec<PathBuf>> {
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

/// Pull HTML documents from one reader into `docs`; returns `true` if `limit` was reached.
fn consume<R: BufRead>(
    r: &mut R,
    docs: &mut Vec<(String, String)>,
    stats: &mut SourceStats,
    seen: &mut HashSet<String>,
    wordlist: Option<&HashSet<String>>,
    limit: Option<usize>,
) -> Result<bool> {
    while let Some(rec) = read_record(r)? {
        if let Some(lim) = limit
            && docs.len() >= lim
        {
            return Ok(true);
        }
        let Some(body) = html_body(&rec) else {
            continue;
        };
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
        docs.push((
            rec.target_uri.clone(),
            String::from_utf8_lossy(body).into_owned(),
        ));
    }
    Ok(false)
}

impl WarcSource {
    pub(crate) fn open(
        input: &Path,
        wordlist: Option<&HashSet<String>>,
        limit: Option<usize>,
    ) -> Result<Self> {
        let mut docs: Vec<(String, String)> = Vec::new();
        let mut stats = SourceStats::default();
        let mut seen: HashSet<String> = HashSet::new();

        for file in gather_warc_files(input)? {
            let f = File::open(&file)
                .map_err(|e| anyhow!("could not open WARC {}: {e}", file.display()))?;
            let reader = BufReader::new(f);
            let hit_limit = if has_suffix(&file, ".gz") {
                let mut r = BufReader::new(MultiGzDecoder::new(reader));
                consume(&mut r, &mut docs, &mut stats, &mut seen, wordlist, limit)?
            } else {
                let mut r = reader;
                consume(&mut r, &mut docs, &mut stats, &mut seen, wordlist, limit)?
            };
            if hit_limit {
                break;
            }
        }

        stats.prepared = docs.len();
        let runs = docs
            .chunks(BUNDLE_CAP)
            .map(|c| c.to_vec())
            .collect::<Vec<_>>();
        Ok(Self { runs, stats })
    }
}

impl Source for WarcSource {
    fn run_count(&self) -> usize {
        self.runs.len()
    }

    fn drive_run(&self, rank: usize, sink: &mut dyn DocSink) -> u32 {
        for (key, html) in &self.runs[rank] {
            sink.accept(key, html);
        }
        0 // read failures were already accounted for during pass 1
    }

    fn stats(&self) -> &SourceStats {
        &self.stats
    }
}
