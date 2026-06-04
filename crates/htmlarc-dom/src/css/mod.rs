#![allow(unused)]

mod chars;
mod ext;
mod helpers;
mod parse;
mod patterns;
mod selectors;
#[cfg(test)]
mod tests;

mod logging {
    pub use crate::logging::debug;
}

pub use chars::CssChars;
pub(super) use logging::debug;
pub use parse::parse_css;
pub use patterns::*;
pub use selectors::*;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("{0}")]
    Parsing(Box<dyn IndexedError>),
    #[error("{0} -> {1}")]
    Context(Box<dyn IndexedError>, Box<Self>),
    #[error("No selector found")]
    EmptySelector,
}

impl ParseError {
    pub fn new<E: IndexedError + 'static>(error: E) -> Self {
        ParseError::Parsing(Box::new(error))
    }
    pub fn context<E: IndexedError + 'static>(self, context: E) -> Self {
        ParseError::Context(Box::new(context), Box::new(self))
    }
}

type ParseResult<T> = Result<T, ParseError>;

pub trait Context {
    fn context<E: IndexedError + 'static>(self, context: E) -> Self;
}

impl<T> Context for ParseResult<T> {
    fn context<E: IndexedError + 'static>(self, context: E) -> Self {
        if let Err(e) = self {
            Err(ParseError::Context(Box::new(context), e.into()))
        } else {
            self
        }
    }
}

pub trait IndexedError: std::error::Error {
    fn index(&self) -> usize;
}

impl<T> From<T> for Box<dyn IndexedError>
where
    T: IndexedError + 'static,
{
    fn from(val: T) -> Self {
        Box::new(val)
    }
}

pub trait Diagnostic {
    fn diagnosis(self, string: &str) -> String;
}

impl Diagnostic for ParseError {
    fn diagnosis(self, string: &str) -> String {
        let mut errors = Vec::new();
        let mut cursor = self;

        loop {
            match cursor {
                ParseError::Parsing(err) => {
                    errors.push((err.index(), err.to_string()));
                    break;
                }
                ParseError::Context(err, context) => {
                    errors.push((err.index(), err.to_string()));
                    cursor = *context;
                }
                ParseError::EmptySelector => break,
            }
        }

        let mut output = String::new();
        let mut last_index = 0;
        for error in errors.iter() {
            if last_index <= error.0 {
                output.push_str(&format!(
                    "{}\u{001b}[4m{}\u{001b}[0m",
                    string[last_index..error.0].to_owned(),
                    string[error.0..=error.0].to_owned()
                ));
                last_index = error.0 + 1;
            }
        }
        output.push_str(&string[last_index..]);
        output.push('\n');

        let mut error_count = errors.len();
        for v_error in errors.iter().rev() {
            let mut last_index = None;
            let mut line = String::new();
            for (i, h_error) in errors.iter().enumerate() {
                if h_error.0 <= v_error.0 {
                    if let Some(last) = last_index {
                        let spaces = " ".repeat(h_error.0.saturating_sub(last + 1));
                        if i == error_count.saturating_sub(1) {
                            if last == h_error.0 {
                                line.pop();
                                line.push('├');
                            } else {
                                line.push_str(&spaces);
                                line.push('└');
                            }
                            let dashes = "─".repeat(string.len().saturating_sub(h_error.0));
                            line.push_str(&dashes);
                            line.push(' ');
                            line.push_str(&h_error.1);
                        } else if h_error.0 > last {
                            line.push_str(&spaces);
                            line.push('│');
                        }
                    } else {
                        let spaces = " ".repeat(h_error.0.saturating_sub(1));
                        if i == error_count.saturating_sub(1) {
                            line.push('└');
                            let dashes = "─".repeat(string.len().saturating_sub(h_error.0));
                            line.push_str(&dashes);
                            line.push(' ');
                            line.push_str(&h_error.1);
                        } else {
                            line.push('│');
                        }
                    }
                    last_index = Some(h_error.0);
                }
            }
            output.push_str(&line);
            output.push('\n');
            error_count = error_count.saturating_sub(1);
        }

        output
    }
}
