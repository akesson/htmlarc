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

        /// Evaluate per-bundle string-block compression framings on a built .htmlarc: sweep
        /// docs/slice × zstd level (and a per-bundle trained dict) over the real BundleStrings
        /// block, reporting size/ratio and random (cold get) vs sequential (seq scan) decode
        /// cost. A read-only probe — it never writes a new format. Build the archive with
        /// `convert` first. Override the grids with FRAMING_SLICES / FRAMING_LEVELS env vars.
        cmd framing {
            /// The .htmlarc archive(s) to analyse. Pass several to pool their documents into one
            /// analysis — e.g. an English and a Chinese archive, to study a multilingual mix.
            repeated input: PathBuf
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
            /// Also measure per-bundle zstd compression of Lane A vs Lane B (slow).
            optional --compress
            /// Also measure the node-topology packing ceiling — parses each document with
            /// the real parser and reports link-delta redundancy (slow; ADR 0002).
            optional --topology
        }
    }
}
