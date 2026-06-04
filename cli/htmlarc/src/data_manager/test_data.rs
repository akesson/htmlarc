use std::{path::Path, sync::Arc};

use anyhow::Result;
use htmlarc_dom::prelude::HtmlDoc;
use htmlarc_format::{HtmlArchive, HtmlEntry};

use super::DataManager;

pub struct TestData;

impl DataManager for TestData {
    fn create_list_arch(&self, _source: &Path) -> Result<Arc<HtmlArchive>> {
        let data: Vec<(&'static str, &'static str)> = vec![
            ("Zephyr", "<body><h2 id='test'>zephyr</h2></body>"),
            ("Ephemeral", "<body><h1 class='test'>ephemeral</h1></body>"),
            ("Galvanize", "<body><h1 id='test'>galvanize</h1></body>"),
            (
                "Obfuscate",
                "<body><h2 id='hello' class='test'>obfuscate</h2></body>",
            ),
            ("Resilient", "<body><h1>resilient</h1></body>"),
        ];

        let entries = data
            .into_iter()
            .map(|(word, html)| {
                let html = HtmlDoc::parse(html).unwrap();
                HtmlEntry::new(word.to_string(), html)
            })
            .collect::<Vec<_>>();

        Ok(Arc::new(HtmlArchive::from_vec(entries)))
    }

    fn create_diff_arch(&self, _source: &Path) -> Result<HtmlArchive> {
        let data: Vec<(&'static str, &'static str)> = vec![
            ("Zephyr", "<body><span>zephyr</span></body>"),
            ("Ephemeral", "<body><h1>ephemeral</h1></body>"),
            ("Galvanize", "<body><span>galvanize</span></body>"),
            ("Obfuscate", "<body><h2>obfuscate</h2></body>"),
            ("Resilient", "<body><h1>resilient</h1></body>"),
        ];

        let entries = data
            .into_iter()
            .map(|(word, html)| {
                let html = HtmlDoc::parse(html).unwrap();
                HtmlEntry::new(word.to_string(), html)
            })
            .collect::<Vec<_>>();

        Ok(HtmlArchive::from_vec(entries))
    }
}
