use crate::args::Pack;
use anyhow::{Context, Result};
use htmlarc_archive::HtmlArchive;

/// Parse a source (file or directory of HTML) and write it out as a single `.htmlarc`,
/// streaming one document at a time so a large directory never goes fully resident.
pub fn run(args: Pack) -> Result<()> {
    let Pack { source, output } = args;

    let count = HtmlArchive::pack_to(&source, &output)
        .with_context(|| format!("packing {} into {}", source.display(), output.display()))?;

    println!("Packed {count} document(s) into {}", output.display());
    Ok(())
}
