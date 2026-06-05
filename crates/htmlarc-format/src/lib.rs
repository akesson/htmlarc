mod archive;
mod builder;
mod entry;
mod error;
mod filter;
mod header;
mod mmap;

pub use archive::HtmlArchive;
pub use builder::HtmlArchiveBuilder;
pub use entry::{ArchivedHtmlEntry, HtmlEntry};
pub use error::ArchiveErr;
pub use filter::Filter;
pub use mmap::MmapArchive;
