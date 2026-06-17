use std::{
    fs,
    io::{Stdout, Write, stdin, stdout},
    path::Path,
};

use anyhow::Result;
use htmlarc_dom::prelude::*;
use termion::{
    event::Key,
    input::TermRead,
    raw::{IntoRawMode, RawTerminal},
};

use super::{DataOperator, OperationError, html_stem};
use crate::source::ArchiveSource;

pub struct FileOperator;

impl FileOperator {
    pub fn new() -> Self {
        Self
    }

    fn navigate<F>(action: F, indexes: &[usize], archive: &ArchiveSource) -> Result<()>
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
        archive: &ArchiveSource,
    ) -> Result<()> {
        let count = indexes.len();
        let prev_idx = (index + count - 1) % count;
        let next_idx = (index + 1) % count;
        let prev_word = archive.key(prev_idx);
        let curr_word = archive.key(index);
        let next_word = archive.key(next_idx);

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
        list_arch: &ArchiveSource,
        diff_arch: &ArchiveSource,
        raw_html: bool,
    ) -> Result<()> {
        fs::create_dir_all(folder)?;

        if indexes.is_empty() {
            println!("No words found for diff");

            return Ok(());
        }

        let fmt = HtmlFormat::raw_else_pretty(raw_html);
        for i in indexes {
            let word = diff_arch.key(*i);
            let html_1 = list_arch
                .html_for_key(word, fmt)?
                .ok_or_else(|| OperationError::GetEntry(word.to_string(), "list archive"))?;
            let html_2 = diff_arch.to_html(*i, fmt);

            let stem = html_stem(word);
            let word_path_1 = folder.join(format!("{stem}.1.html"));
            let word_path_2 = folder.join(format!("{stem}.2.html"));

            fs::write(word_path_1, html_1)?;
            fs::write(word_path_2, html_2)?;
        }

        Ok(())
    }

    fn write_list(
        &mut self,
        folder: &Path,
        indexes: &[usize],
        archive: &ArchiveSource,
        raw_html: bool,
    ) -> Result<()> {
        fs::create_dir_all(folder)?;

        if indexes.is_empty() {
            println!("No words found");

            return Ok(());
        }

        let fmt = HtmlFormat::raw_else_pretty(raw_html);
        for i in indexes {
            let word_path = folder.join(format!("{}.html", html_stem(archive.key(*i))));
            fs::write(word_path, archive.to_html(*i, fmt))?;
        }

        Ok(())
    }

    fn navigate_diff(
        &mut self,
        indexes: &[usize],
        list_arch: &ArchiveSource,
        diff_arch: &ArchiveSource,
        raw_html: bool,
    ) -> Result<()> {
        Self::navigate(
            |index| {
                let i = indexes[index];
                let word = diff_arch.key(i);
                let fmt = HtmlFormat::raw_else_pretty(raw_html);
                let html_1 = list_arch
                    .html_for_key(word, fmt)?
                    .ok_or_else(|| OperationError::GetEntry(word.to_string(), "archive 1"))?;
                let html_2 = diff_arch.to_html(i, fmt);

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
        archive: &ArchiveSource,
        raw_html: bool,
    ) -> Result<()> {
        Self::navigate(
            |index| {
                let i = indexes[index];
                let fmt = HtmlFormat::raw_else_pretty(raw_html);
                fs::write("word.html", archive.to_html(i, fmt))?;

                Ok(())
            },
            indexes,
            archive,
        )?;

        Ok(())
    }

    fn list(&mut self, indexes: &[usize], archive: &ArchiveSource) {
        if indexes.is_empty() {
            println!("No words found");

            return;
        }
        for index in indexes {
            println!("{}", archive.key(*index));
        }
        println!("Found {} word(s):", indexes.len());
    }
}
