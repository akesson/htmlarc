//! The `list` and `extract` inspection commands, thin wrappers over any
//! [`Source`](crate::source::Source).

use anyhow::{Result, bail};

use crate::args::{Extract, List};
use crate::source::{DocSink, open_source};

/// Prints each document's key, one per line.
struct ListSink;

impl DocSink for ListSink {
    fn accept(&mut self, key: &str, _html: &str) {
        println!("{key}");
    }
}

pub(crate) fn list(args: List) -> Result<()> {
    let source = open_source(&args.input, args.format.as_deref(), None, None)?;
    let mut sink = ListSink;
    for rank in 0..source.run_count() {
        source.drive_run(rank, &mut sink);
    }
    Ok(())
}

/// Captures the HTML of the first document whose key matches exactly.
struct ExtractSink<'a> {
    want: &'a str,
    html: Option<String>,
}

impl DocSink for ExtractSink<'_> {
    fn accept(&mut self, key: &str, html: &str) {
        if self.html.is_none() && key == self.want {
            self.html = Some(html.to_string());
        }
    }
}

pub(crate) fn extract(args: Extract) -> Result<()> {
    let source = open_source(&args.input, args.format.as_deref(), None, None)?;
    let mut sink = ExtractSink {
        want: &args.key,
        html: None,
    };
    for rank in 0..source.run_count() {
        source.drive_run(rank, &mut sink);
        if sink.html.is_some() {
            break;
        }
    }
    match sink.html {
        Some(html) => {
            print!("{html}");
            Ok(())
        }
        None => bail!("no document with key '{}' found", args.key),
    }
}
