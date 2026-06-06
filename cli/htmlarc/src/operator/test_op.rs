use std::path::Path;

use anyhow::Result;
use htmlarc_dom::prelude::*;
use sanitize_filename::sanitize;

use super::{DataOperator, OperationError};
use crate::source::ArchiveSource;

#[derive(Default)]
pub struct TestOperator {
    output: String,
}

impl TestOperator {
    pub fn new() -> Self {
        Self::default()
    }

    fn add_output(&mut self, output: &str) {
        self.output.push_str(output);
    }

    pub fn string(&self) -> String {
        self.output.clone()
    }
}

impl DataOperator for TestOperator {
    fn write_diff_list(
        &mut self,
        _folder: &Path,
        indexes: &[usize],
        list_arch: &ArchiveSource,
        diff_arch: &ArchiveSource,
        raw_html: bool,
    ) -> Result<()> {
        self.add_output("Write Diff:");

        if indexes.is_empty() {
            self.add_output("[empty]");

            return Ok(());
        }

        let fmt = HtmlFormat::raw_else_pretty(raw_html);
        for i in indexes {
            let word = diff_arch.key(*i);
            let html_1 = list_arch
                .html_for_key(word, fmt)?
                .ok_or_else(|| OperationError::GetEntry(word.to_string(), "list archive"))?;
            let html_2 = diff_arch.to_html(*i, fmt);

            let sanitized = sanitize(word);
            self.add_output(&format!("\n\n{sanitized}:\n[\n{html_1}\n-\n{html_2}\n]"));
        }

        Ok(())
    }

    fn write_list(
        &mut self,
        _folder: &Path,
        indexes: &[usize],
        archive: &ArchiveSource,
        raw_html: bool,
    ) -> Result<()> {
        self.add_output("Write List:");

        if indexes.is_empty() {
            self.add_output("[empty]");

            return Ok(());
        }

        let fmt = HtmlFormat::raw_else_pretty(raw_html);
        for i in indexes {
            let sanitized = sanitize(archive.key(*i));
            let html = archive.to_html(*i, fmt);

            self.add_output(&format!("\n\n{sanitized}:\n[\n{html}\n]"));
        }

        Ok(())
    }

    fn navigate_diff(
        &mut self,
        indexes: &[usize],
        _list_arch: &ArchiveSource,
        diff_arch: &ArchiveSource,
        _raw_html: bool,
    ) -> Result<()> {
        self.add_output(&format!(
            "Navigate diff {} word(s): {}",
            indexes.len(),
            indexes
                .iter()
                .map(|i| diff_arch.key(*i).to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));

        Ok(())
    }

    fn navigate_list(
        &mut self,
        indexes: &[usize],
        archive: &ArchiveSource,
        _raw_html: bool,
    ) -> Result<()> {
        self.add_output(&format!(
            "Navigate {} word(s): {}",
            indexes.len(),
            indexes
                .iter()
                .map(|i| archive.key(*i).to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));

        Ok(())
    }

    fn list(&mut self, indexes: &[usize], archive: &ArchiveSource) {
        self.add_output("List:");

        if indexes.is_empty() {
            self.add_output("[empty]");

            return;
        }

        for index in indexes {
            self.add_output("\n");
            self.add_output(archive.key(*index));
        }
    }
}
