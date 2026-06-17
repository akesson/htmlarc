#[cfg(not(test))]
mod file_op;
#[cfg(not(test))]
pub use file_op::FileOperator as Operator;

#[cfg(test)]
mod test_op;
#[cfg(test)]
pub use test_op::TestOperator as Operator;

use std::path::Path;

use anyhow::Result;
use sanitize_filename::sanitize;

use crate::source::ArchiveSource;

#[derive(Debug, thiserror::Error)]
pub(super) enum OperationError {
    #[error("'{0}' word not found in '{1}'")]
    GetEntry(String, &'static str),
}

/// Build the on-disk file stem for a document key: sanitize it for the filesystem, then drop a
/// redundant trailing `.html`/`.htm` so a key that already names an HTML file (a directory source
/// keyed by file name, a WARC URL ending in `.html`) does not become `name.html.html`. Callers
/// append the real extension (`.html`, or `.1.html`/`.2.html` for diffs).
///
/// Lives here (not in `file_op`, which is `#[cfg(not(test))]`) so it can be unit-tested.
fn html_stem(key: &str) -> String {
    let s = sanitize(key);
    let lower = s.to_ascii_lowercase();
    for ext in [".html", ".htm"] {
        if lower.ends_with(ext) {
            return s[..s.len() - ext.len()].to_string();
        }
    }
    s
}

pub trait DataOperator {
    fn write_diff_list(
        &mut self,
        folder: &Path,
        indexes: &[usize],
        list_arch: &ArchiveSource,
        diff_arch: &ArchiveSource,
        raw_html: bool,
    ) -> Result<()>;
    fn write_list(
        &mut self,
        folder: &Path,
        indexes: &[usize],
        archive: &ArchiveSource,
        raw_html: bool,
    ) -> Result<()>;
    fn navigate_diff(
        &mut self,
        indexes: &[usize],
        list_arch: &ArchiveSource,
        diff_arch: &ArchiveSource,
        raw_html: bool,
    ) -> Result<()>;
    fn navigate_list(
        &mut self,
        indexes: &[usize],
        archive: &ArchiveSource,
        raw_html: bool,
    ) -> Result<()>;
    fn list(&mut self, indexes: &[usize], archive: &ArchiveSource);
}

#[cfg(test)]
mod tests {
    use super::html_stem;

    #[test]
    fn html_stem_avoids_double_html_extension() {
        // A key that already names an HTML file: the caller's `.html` must not double up.
        assert_eq!(html_stem("doc3.html"), "doc3");
        assert_eq!(format!("{}.html", html_stem("doc3.html")), "doc3.html");
        // Case-insensitive, and `.htm` too.
        assert_eq!(html_stem("Page.HTML"), "Page");
        assert_eq!(html_stem("a.htm"), "a");
        // Keys without an HTML extension are kept (then get `.html` appended by the caller).
        assert_eq!(html_stem("Some Title"), "Some Title");
        assert_eq!(html_stem("noext"), "noext");
        // Diff variant: `.1.html` is appended to the stem, not after the existing extension.
        assert_eq!(format!("{}.1.html", html_stem("doc3.html")), "doc3.1.html");
    }
}
