mod args;
mod commands;
mod convert;
mod framing;
mod source;
mod stats;
#[cfg(test)]
mod tests;

use std::process::ExitCode;

use anyhow::Result;

use args::{HtmlarcConvert, HtmlarcConvertCmd};

// Allocation profiling needs every allocation routed through hotpath's counting allocator.
#[cfg(feature = "hotpath-alloc")]
#[global_allocator]
static GLOBAL: hotpath::CountingAllocator<std::alloc::System> = hotpath::CountingAllocator::new();

fn main() -> ExitCode {
    // Held for the whole run; on drop it prints the allocation report. `functions_limit(0)`
    // shows every measured label rather than truncating to the top N.
    #[cfg(feature = "hotpath")]
    let _hotpath = hotpath::HotpathGuardBuilder::new("htmlarc-convert")
        .functions_limit(0)
        .build();
    match run_cli() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run_cli() -> Result<()> {
    let cli = HtmlarcConvert::from_env()?;
    match cli.subcommand {
        HtmlarcConvertCmd::List(a) => commands::list(a),
        HtmlarcConvertCmd::Extract(a) => commands::extract(a),
        HtmlarcConvertCmd::Convert(a) => convert::run(a),
        HtmlarcConvertCmd::Framing(a) => framing::run(a),
        HtmlarcConvertCmd::Stats(a) => stats::run(a),
    }
}
