use std::path::Path;

use anyhow::Result;
use htmlarc_dom::prelude::*;
use htmlarc_format::HtmlArchive;
use sanitize_filename::sanitize;

use super::{DataOperator, OperationError};

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
        list_arch: &HtmlArchive,
        diff_arch: &HtmlArchive,
        raw_html: bool,
    ) -> Result<()> {
        self.add_output("Write Diff:");

        if indexes.is_empty() {
            self.add_output("[empty]");

            return Ok(());
        }

        for i in indexes {
            let entry_2 = &diff_arch[*i];
            let word = &entry_2.key;
            let entry_1 = list_arch
                .get(word)
                .ok_or(OperationError::GetEntry(word.clone(), "list archive"))?;

            let sanitized = sanitize(word);
            let fmt = HtmlFormat::raw_else_pretty(raw_html);
            let html_1 = entry_1.html.to_html(fmt);
            let html_2 = entry_2.html.to_html(fmt);

            self.add_output(&format!("\n\n{sanitized}:\n[\n{html_1}\n-\n{html_2}\n]"));
        }

        Ok(())
    }

    fn write_list(
        &mut self,
        _folder: &Path,
        indexes: &[usize],
        archive: &HtmlArchive,
        raw_html: bool,
    ) -> Result<()> {
        self.add_output("Write List:");

        if indexes.is_empty() {
            self.add_output("[empty]");

            return Ok(());
        }

        for i in indexes {
            let entry = &archive[*i];

            let sanitized = sanitize(&entry.key);

            let fmt = HtmlFormat::raw_else_pretty(raw_html);
            let html = entry.html.to_html(fmt);

            self.add_output(&format!("\n\n{sanitized}:\n[\n{html}\n]"));
        }

        Ok(())
    }

    fn navigate_diff(
        &mut self,
        indexes: &[usize],
        _list_arch: &HtmlArchive,
        diff_arch: &HtmlArchive,
        _raw_html: bool,
    ) -> Result<()> {
        self.add_output(&format!(
            "Navigate diff {} word(s): {}",
            indexes.len(),
            indexes
                .iter()
                .map(|i| diff_arch[*i].key.clone())
                .collect::<Vec<_>>()
                .join(", ")
        ));

        Ok(())
    }

    fn navigate_list(
        &mut self,
        indexes: &[usize],
        archive: &HtmlArchive,
        _raw_html: bool,
    ) -> Result<()> {
        self.add_output(&format!(
            "Navigate {} word(s): {}",
            indexes.len(),
            indexes
                .iter()
                .map(|i| archive[*i].key.clone())
                .collect::<Vec<_>>()
                .join(", ")
        ));

        Ok(())
    }

    fn list(&mut self, indexes: &[usize], archive: &HtmlArchive) {
        self.add_output("List:");

        if indexes.is_empty() {
            self.add_output("[empty]");

            return;
        }

        for index in indexes {
            self.add_output("\n");
            self.add_output(&archive[*index].key);
        }
    }
}
