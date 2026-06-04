mod args;
mod export;
#[cfg(test)]
mod tests;

use std::process::ExitCode;

use anyhow::{Result, anyhow};
use zim::Target;

use args::{Extract, List, Zim2htmlarc, Zim2htmlarcCmd};

fn main() -> ExitCode {
    match run_cli() {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run_cli() -> Result<()> {
    let cli = Zim2htmlarc::from_env()?;
    match cli.subcommand {
        Zim2htmlarcCmd::List(a) => run_list(a),
        Zim2htmlarcCmd::Extract(a) => run_extract(a),
        Zim2htmlarcCmd::Export(a) => export::run(a),
    }
}

/// List every HTML article as `title<TAB>url`.
fn run_list(args: List) -> Result<()> {
    let zim = export::open(&args.file)?;
    for entry in zim.iterate_by_urls() {
        if export::is_content(&entry.namespace)
            && matches!(entry.target, Some(Target::Cluster(_, _)))
            && export::html_mime(&entry.mime_type)
        {
            println!(
                "{}\t{}",
                export::key_for(&entry.title, &entry.url),
                entry.url
            );
        }
    }
    Ok(())
}

/// Print the HTML of the article whose title matches exactly (NFC-compared).
///
/// Note: the original libzim-based tool did a Xapian full-text search here; the pure-Rust
/// `zim` crate has no search, so this is an exact-title match (a linear scan).
fn run_extract(args: Extract) -> Result<()> {
    let zim = export::open(&args.file)?;
    let want = export::nfc(&args.title);
    for entry in zim.iterate_by_urls() {
        if !export::is_content(&entry.namespace)
            || !export::html_mime(&entry.mime_type)
            || export::key_for(&entry.title, &entry.url) != want
        {
            continue;
        }
        if let Some(Target::Cluster(c, b)) = entry.target {
            let cluster = zim
                .get_cluster(c)
                .map_err(|e| anyhow!("get_cluster({c}) failed: {e:?}"))?;
            let blob = cluster
                .get_blob(b)
                .map_err(|e| anyhow!("get_blob({b}) failed: {e:?}"))?;
            print!("{}", String::from_utf8_lossy(blob.as_ref()));
            return Ok(());
        }
    }
    Err(anyhow!("no content article titled '{}' found", args.title))
}
