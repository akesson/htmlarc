mod args;
mod commands;
mod convert;
mod source;
mod stats;
#[cfg(test)]
mod tests;

use std::process::ExitCode;

use anyhow::Result;

use args::{HtmlarcConvert, HtmlarcConvertCmd};

fn main() -> ExitCode {
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
        HtmlarcConvertCmd::Stats(a) => stats::run(a),
    }
}
