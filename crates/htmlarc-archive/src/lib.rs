mod append;
mod archive;
mod archive_trait;
mod builder;
mod bundle;
mod bundle_strings;
mod codec;
mod doc_table;
mod entry;
mod error;
mod filter;
mod header;
mod meta;
mod mmap;
mod trailer;
mod writer;

pub use append::ArchiveAppender;
pub use archive::HtmlArchive;
pub use archive_trait::{Archive, ArchiveEntry};
pub use builder::HtmlArchiveBuilder;
pub use bundle::{BUNDLE_CAP, DocBundle};
pub use codec::{StringCompressor, StringEncoder, train_string_dict};
pub use entry::{ArchivedHtmlEntry, HtmlEntry, SerializedEntry};
pub use error::ArchiveErr;
pub use filter::{Filter, FilterError};
pub use meta::{
    ArchivedMetaColumn, ArchivedMetaTable, MetaColumn, MetaRef, MetaSchema, MetaTable,
    MetaTableBuilder, MetaType, MetaValue, archived_value,
};
pub use mmap::{Doc, MmapArchive, OwnedDoc};
pub use writer::ArchiveWriter;
