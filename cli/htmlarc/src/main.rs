mod args;
mod data_manager;
mod helpers;
mod operator;
mod pack;
mod probe;
mod superscript;
#[cfg(test)]
mod tests;

mod logging {
    #[cfg(not(test))]
    pub use log::debug;
    #[cfg(test)]
    pub use std::println as debug;
}

// Crate-root items the `probe` modules expect (mirrors the original htmlprobe lib.rs).
pub(crate) use anyhow::{Error, Result, anyhow};
pub(crate) use htmlarc_dom::css::{SelectorList, parse_css};
pub(crate) use htmlarc_format::{Filter, HtmlArchive, HtmlEntry};
pub(crate) use logging::debug;
pub use probe::*;

pub use helpers::read_arch;

use std::process::ExitCode;

use args::{Diff, Htmlarc, HtmlarcCmd, List};
use data_manager::{DataManager, Manager};
use helpers::{create_diff_indexes, create_list_indexes};
use operator::{DataOperator, Operator};

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
    let cli = Htmlarc::from_env()?;
    let mut operator = Operator::new();
    let data_manager = Manager;
    process(&mut operator, data_manager, cli)
}

/// Dispatch a parsed command. Generic over the operator/data-manager so tests can inject
/// in-memory fixtures instead of touching the filesystem and terminal.
pub fn process<O: DataOperator, D: DataManager>(
    operator: &mut O,
    data_manager: D,
    cli: Htmlarc,
) -> Result<()> {
    match cli.subcommand {
        HtmlarcCmd::List(a) => run_list(operator, data_manager, a),
        HtmlarcCmd::Diff(a) => run_diff(operator, data_manager, a),
        HtmlarcCmd::Pack(a) => pack::run(a),
        HtmlarcCmd::Probe(a) => probe::run(a),
    }
}

fn run_list<O: DataOperator, D: DataManager>(
    operator: &mut O,
    data_manager: D,
    args: List,
) -> Result<()> {
    let List {
        source,
        include,
        exclude,
        first_n,
        raw_html,
        to_folder,
        navigate,
    } = args;

    let archive = data_manager.create_list_arch(&source)?;
    let indexes = create_list_indexes(&archive, include, exclude, first_n)?;

    let default = to_folder.is_none() && !navigate;
    if let Some(folder) = &to_folder {
        operator.write_list(folder, &indexes, &archive, raw_html)?;
    }
    if navigate {
        operator.navigate_list(&indexes, &archive, raw_html)?;
    }
    if default {
        operator.list(&indexes, &archive);
    }
    Ok(())
}

fn run_diff<O: DataOperator, D: DataManager>(
    operator: &mut O,
    data_manager: D,
    args: Diff,
) -> Result<()> {
    let Diff {
        source,
        other,
        raw_html,
        to_folder,
        navigate,
    } = args;

    let list_archive = data_manager.create_list_arch(&source)?;
    let diff_archive = data_manager.create_diff_arch(&other)?;
    let diff_indexes = create_diff_indexes(&list_archive, &diff_archive);

    let default = to_folder.is_none() && !navigate;
    if let Some(folder) = &to_folder {
        operator.write_diff_list(folder, &diff_indexes, &list_archive, &diff_archive, raw_html)?;
    }
    if navigate {
        operator.navigate_diff(&diff_indexes, &list_archive, &diff_archive, raw_html)?;
    }
    if default {
        operator.list(&diff_indexes, &diff_archive);
    }
    Ok(())
}
