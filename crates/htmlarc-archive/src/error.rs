use thiserror::Error;

#[derive(Debug, Error)]
pub enum ArchiveErr {
    #[error("Failed to parse HTML {0}")]
    Parse(String),
    #[error("Failed to serialize archive: {0}")]
    Serialize(String),
    #[error("Failed to deserialize archive: {0}")]
    Deserialize(String),
    #[error("Invalid .htmlarc header: {0}")]
    Header(String),
    #[error("Failed to validate archive: {0}")]
    Validate(String),
    #[error("Failed to read archive: {0}")]
    FileRead(std::io::Error),
    #[error("Failed to write archive: {0}")]
    FileWrite(std::io::Error),
}
