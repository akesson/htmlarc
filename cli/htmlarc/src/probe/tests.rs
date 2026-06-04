use std::{
    fs::{self, File},
    io::Write,
    process::{Command, Stdio},
};

use htmlarc_dom::prelude::HtmlDoc;
use htmlarc_format::HtmlEntry;
use insta::assert_snapshot;
use rkyv::{api::high::to_bytes_in, rancor::Error};

#[test]
fn probe_integration_test() {
    let archive = create_archive();

    let cmd = Command::new("cargo")
        .args([
            "htmlprobe",
            archive,
            "-p",
            "div, section, h2, h3, table, ol => HtmlFmt[id][class]",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let output = cmd.wait_with_output().unwrap();

    fs::remove_file(archive).unwrap();

    assert_snapshot!(format!("{}", String::from_utf8_lossy(&output.stdout)).remove_duration());
}

trait RemoveDuration {
    fn remove_duration(self) -> Self;
}

impl RemoveDuration for String {
    fn remove_duration(self) -> Self {
        const DURATION_STR: &str = "Duration";
        if let Some(duration_start) = self.find(DURATION_STR) {
            self[..duration_start].to_string()
        } else {
            self
        }
    }
}

fn create_archive<'a>() -> &'a str {
    const DATA_DIR: &str = "src/testdata";
    const ARCHIVE_PATH: &str = "src/testdata/test_archive.htmlarc";

    let mut files: Vec<_> = fs::read_dir(DATA_DIR)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|p| p.is_file())
        .collect();

    files.sort();

    let entries: Vec<_> = files
        .iter()
        .map(|path| {
            let file_name = path.file_stem().unwrap();
            let word = file_name.to_str().unwrap().to_string();
            let content = fs::read_to_string(path).unwrap();
            let html = HtmlDoc::parse(&content).unwrap();

            HtmlEntry::new(word, html)
        })
        .collect();

    let arch_data = to_bytes_in::<_, Error>(&entries, Vec::new()).unwrap();

    let mut arch_file = File::create(ARCHIVE_PATH).unwrap();

    arch_file.write_all(&arch_data).unwrap();

    ARCHIVE_PATH
}
