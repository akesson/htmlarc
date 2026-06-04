use std::{
    fs,
    io::{Stdout, Write, stdin, stdout},
    path::Path,
};

use anyhow::Result;
use htmlarc_dom::prelude::*;
use htmlarc_format::HtmlArchive;
use sanitize_filename::sanitize;
use termion::{
    event::Key,
    input::TermRead,
    raw::{IntoRawMode, RawTerminal},
};

use super::{DataOperator, OperationError};

pub struct FileOperator;

impl FileOperator {
    pub fn new() -> Self {
        Self
    }

    fn navigate<F>(action: F, indexes: &[usize], archive: &HtmlArchive) -> Result<()>
    where
        F: Fn(usize) -> Result<()>,
    {
        let stdin = stdin();
        let mut stdout = stdout().into_raw_mode()?;

        let count = indexes.len();

        if count < 1 {
            write!(stdout, "No words found for navigation",)?;

            stdout.flush()?;

            return Ok(());
        }

        let mut index: usize = 0;

        action(index)?;

        Self::instructions(&mut stdout, index, indexes, archive)?;

        for c in stdin.keys() {
            Self::instructions(&mut stdout, index, indexes, archive)?;
            action(index)?;

            match c.unwrap() {
                Key::Left => {
                    index = (index + count - 1) % count;
                }
                Key::Right => {
                    index = (index + 1) % count;
                }
                Key::Esc | Key::Ctrl('c') => {
                    // exit
                    break;
                }
                _ => (),
            }
        }

        Ok(())
    }

    fn instructions(
        stdout: &mut RawTerminal<Stdout>,
        index: usize,
        indexes: &[usize],
        archive: &HtmlArchive,
    ) -> Result<()> {
        let count = indexes.len();
        let prev_idx = (index + count - 1) % count;
        let next_idx = (index + 1) % count;
        let prev_word = &archive[prev_idx].key;
        let curr_word = &archive[index].key;
        let next_word = &archive[next_idx].key;

        write!(
            stdout,
            "{}{}word count: {}{}",
            termion::clear::All,
            termion::cursor::Goto(1, 1),
            count,
            termion::cursor::Hide
        )?;
        write!(
            stdout,
            "{}(<-) previous word: [{}]{}",
            termion::cursor::Goto(1, 2),
            prev_word,
            termion::cursor::Hide
        )?;
        write!(
            stdout,
            "{}current word at {}: [{}]{}",
            termion::cursor::Goto(1, 3),
            index,
            curr_word,
            termion::cursor::Hide
        )?;
        write!(
            stdout,
            "{}(->) next word: [{}]{}",
            termion::cursor::Goto(1, 4),
            next_word,
            termion::cursor::Hide
        )?;
        write!(
            stdout,
            "{}(Esc or Ctrl+C to exit){}",
            termion::cursor::Goto(1, 5),
            termion::cursor::Hide
        )?;
        stdout.flush()?;

        Ok(())
    }
}

impl DataOperator for FileOperator {
    fn write_diff_list(
        &mut self,
        folder: &Path,
        indexes: &[usize],
        list_arch: &HtmlArchive,
        diff_arch: &HtmlArchive,
        raw_html: bool,
    ) -> Result<()> {
        fs::create_dir_all(folder)?;

        if indexes.is_empty() {
            println!("No words found for diff");

            return Ok(());
        }

        for i in indexes {
            let entry_2 = &diff_arch[*i];
            let word = &entry_2.key;
            let entry_1 = list_arch
                .get(word)
                .ok_or(OperationError::GetEntry(word.clone(), "list archive"))?;

            let sanitized = sanitize(word);

            let word_path_1 = folder.join(format!("{}.1.html", sanitized));
            let word_path_2 = folder.join(format!("{}.2.html", sanitized));

            let fmt = HtmlFormat::raw_else_pretty(raw_html);
            let html_1 = entry_1.html.to_html(fmt);
            let html_2 = entry_2.html.to_html(fmt);

            fs::write(word_path_1, html_1)?;
            fs::write(word_path_2, html_2)?;
        }

        Ok(())
    }

    fn write_list(
        &mut self,
        folder: &Path,
        indexes: &[usize],
        archive: &HtmlArchive,
        raw_html: bool,
    ) -> Result<()> {
        fs::create_dir_all(folder)?;

        if indexes.is_empty() {
            println!("No words found");

            return Ok(());
        }

        for i in indexes {
            let entry = &archive[*i];

            let sanitized = sanitize(&entry.key);

            let word_path = folder.join(format!("{}.html", sanitized));

            let fmt = HtmlFormat::raw_else_pretty(raw_html);
            let html = entry.html.to_html(fmt);

            fs::write(word_path, html)?;
        }

        Ok(())
    }

    fn navigate_diff(
        &mut self,
        indexes: &[usize],
        list_arch: &HtmlArchive,
        diff_arch: &HtmlArchive,
        raw_html: bool,
    ) -> Result<()> {
        Self::navigate(
            |index| {
                let i = indexes[index];
                let entry_2 = &diff_arch[i];
                let word = &entry_2.key;
                let entry_1 = list_arch
                    .get(word)
                    .ok_or(OperationError::GetEntry(word.clone(), "archive 1"))?;

                let fmt = HtmlFormat::raw_else_pretty(raw_html);
                let html_1 = entry_1.html.to_html(fmt);
                let html_2 = entry_2.html.to_html(fmt);

                fs::write("diff.1.html", html_1)?;
                fs::write("diff.2.html", html_2)?;

                Ok(())
            },
            indexes,
            diff_arch,
        )?;

        Ok(())
    }

    fn navigate_list(
        &mut self,
        indexes: &[usize],
        archive: &HtmlArchive,
        raw_html: bool,
    ) -> Result<()> {
        Self::navigate(
            |index| {
                let i = indexes[index];
                let entry = &archive[i];

                let fmt = HtmlFormat::raw_else_pretty(raw_html);
                let html = entry.html.to_html(fmt);

                fs::write("word.html", html)?;

                Ok(())
            },
            indexes,
            archive,
        )?;

        Ok(())
    }

    fn list(&mut self, indexes: &[usize], archive: &HtmlArchive) {
        if indexes.is_empty() {
            println!("No words found");

            return;
        }
        for index in indexes {
            println!("{}", archive[*index].key);
        }
        println!("Found {} word(s):", indexes.len());
    }
}
