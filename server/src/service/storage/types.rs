use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("failed to put object: {0}")]
    Put(String),
    #[error("failed to get object: {0}")]
    Get(String),
    #[error("failed to delete object: {0}")]
    Delete(String),
    #[error("object not found")]
    NotFound,
    #[error("configuration error: {0}")]
    Config(String),
    #[error("unknown storage error")]
    Unknown,
    #[error("unsupported operation: {0}")]
    Unsupported(String),
}

pub type StorageResult<T> = Result<T, StorageError>;

#[derive(Debug, Default, Clone)]
pub struct PutObjectOptions<'a> {
    pub content_type: Option<&'a str>,
    pub content_encoding: Option<&'a str>,
}

#[derive(Debug, Default, Clone)]
pub struct GetObjectOptions<'a> {
    pub content_encoding: Option<&'a str>,
}
