use crate::args::Pack;
use anyhow::{Context, Result};
use htmlarc_format::HtmlArchive;

/// Parse a source (file or directory of HTML) and write it out as a single `.htmlarc`.
pub fn run(args: Pack) -> Result<()> {
    let Pack { source, output } = args;

    let archive = HtmlArchive::open(&source)
        .with_context(|| format!("opening source {}", source.display()))?;
    archive
        .write_to(&output)
        .with_context(|| format!("writing archive {}", output.display()))?;

    println!(
        "Packed {} document(s) into {}",
        archive.len(),
        output.display()
    );
    Ok(())
}
