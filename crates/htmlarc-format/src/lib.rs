mod archive;
mod archive_trait;
mod builder;
mod entry;
mod error;
mod filter;
mod header;
mod mmap;

pub use archive::HtmlArchive;
pub use archive_trait::{Archive, ArchiveEntry};
pub use builder::HtmlArchiveBuilder;
pub use entry::{ArchivedHtmlEntry, HtmlEntry};
pub use error::ArchiveErr;
pub use filter::{Filter, FilterError};
pub use mmap::MmapArchive;
