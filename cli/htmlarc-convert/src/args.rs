use std::path::PathBuf;

xflags::xflags! {
    /// Convert ZIM, WARC, or a directory of HTML files into an .htmlarc archive.
    ///
    /// The input format is inferred from the path (`.zim`, `.warc`/`.warc.gz`, or a
    /// directory of HTML files) unless `--format` is given.
    cmd htmlarc-convert {
        /// List every document as `key <tab> source-locator`.
        cmd list {
            /// The input: a .zim, a .warc(.gz), or a directory.
            required input: PathBuf
            /// Force the input format: zim | warc | dir.
            optional --format fmt: String
        }

        /// Print one document's HTML to stdout, found by exact key.
        cmd extract {
            /// The input: a .zim, a .warc(.gz), or a directory.
            required input: PathBuf
            /// The exact document key (ZIM title, WARC target URI, or relative file path).
            required key: String
            /// Force the input format: zim | warc | dir.
            optional --format fmt: String
        }

        /// Convert the input's HTML documents into a single .htmlarc archive.
        cmd convert {
            /// The input: a .zim, a .warc(.gz), or a directory.
            required input: PathBuf
            /// The .htmlarc archive to write.
            required output: PathBuf
            /// Only convert documents whose key is in this file (one key per line).
            optional --list wordlist: PathBuf
            /// Stop after this many documents (useful for sampling a huge input).
            optional --limit count: usize
            /// Force the input format: zim | warc | dir.
            optional --format fmt: String
        }

        /// Probe per-document and per-bundle string/list cardinalities (a tolerant pass
        /// that does not require the documents to parse). Gates the ADR 0002 constants.
        cmd stats {
            /// The input: a .zim, a .warc(.gz), or a directory.
            required input: PathBuf
            /// Stop after this many documents (useful for sampling a huge input).
            optional --limit count: usize
            /// Force the input format: zim | warc | dir.
            optional --format fmt: String
        }
    }
}
