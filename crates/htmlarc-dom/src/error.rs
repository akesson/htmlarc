use std::{
    fmt::{Debug, Display},
    ops::Range,
    panic::Location,
};

use thiserror::Error;

use crate::{
    CodeLocation,
    html::{HtmlElement, HtmlTag},
};

#[derive(Error, Debug, PartialEq, Eq)]
pub enum HtmlParseError {
    #[error("{0}")]
    Parsing(String),
    #[error("{0}\n{1}")]
    WithContext(String, Box<Self>),
}

impl HtmlParseError {
    pub fn new<S: ToString>(msg: S) -> Self {
        HtmlParseError::Parsing(msg.to_string())
    }
}

pub type HtmlParseResult<T> = std::result::Result<T, HtmlParseError>;

#[derive(Error, Debug)]
pub enum IterationError {
    #[error("No elements in the iterator")]
    Empty,
    #[error("[Expected {expected:?}, found more than {max}]")]
    Exceeds { max: usize, expected: Range<usize> },
    #[error("[Expected {expected:?}, found only {count}]")]
    Lacks {
        count: usize,
        expected: Range<usize>,
    },
}

impl IterationError {
    pub fn locate(self, location: CodeLocation) -> LocatedError {
        LocatedError::new(self, location)
    }
}

impl From<IterationError> for LocatedError {
    fn from(value: IterationError) -> Self {
        let location = CodeLocation::File(Location::caller().file());
        let message = value.to_string();
        Self {
            source: message,
            location,
        }
    }
}

#[derive(Error, Debug)]
pub enum ElementError<'dom, Dom> {
    #[error("Element doesn't have a next sibling")]
    NoNextSibling(HtmlElement<'dom, Dom>),
    #[error("Element doesn't have a previous sibling")]
    NoPreviousSibling(HtmlElement<'dom, Dom>),
    #[error("Element doesn't have a parent")]
    NoParent(HtmlElement<'dom, Dom>),
    #[error("Element doesn't have a child")]
    NoChild(HtmlElement<'dom, Dom>),
    #[error("Element doesn't have a child with tag '{1}'")]
    NoChildTag(HtmlElement<'dom, Dom>, HtmlTag),
}

impl<Dom> ElementError<'_, Dom> {
    pub fn locate(self, location: CodeLocation) -> LocatedError {
        LocatedError::new(self, location)
    }
}

pub trait Locatable<Val> {
    fn err_location(self, location: CodeLocation) -> Result<Val, LocatedError>;
}

impl<Dom, Val> Locatable<Val> for Result<Val, ElementError<'_, Dom>> {
    fn err_location(self, location: CodeLocation) -> Result<Val, LocatedError> {
        self.map_err(|e| e.locate(location))
    }
}

impl<Val> Locatable<Val> for Result<Val, IterationError> {
    fn err_location(self, location: CodeLocation) -> Result<Val, LocatedError> {
        self.map_err(|e| e.locate(location))
    }
}

/// This is an error that has already been logged as a comment in the dom,
/// but is used to interrupt the processing.
pub struct LocatedError {
    source: String,
    location: CodeLocation,
}

impl LocatedError {
    pub fn new<S: ToString>(message: S, location: CodeLocation) -> Self {
        Self {
            source: message.to_string(),
            location,
        }
    }
}

impl std::error::Error for LocatedError {}
impl Display for LocatedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at {}", self.source, self.location)
    }
}

impl Debug for LocatedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at {}", self.source, self.location)
    }
}

impl<'dom, Dom> From<ElementError<'dom, Dom>> for LocatedError {
    #[track_caller]
    fn from(value: ElementError<'dom, Dom>) -> Self {
        let location = CodeLocation::File(Location::caller().file());
        let message = value.to_string();
        Self {
            source: message,
            location,
        }
    }
}
