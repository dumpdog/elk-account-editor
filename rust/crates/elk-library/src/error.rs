//! Errors of the library. The `Display` text is what the page shows.

use std::fmt;

#[derive(Debug)]
pub enum Error {
    /// The request itself is wrong (empty name, a folder moved into itself, a name already used ...).
    Invalid(String),
    /// A folder or account that does not exist (any more).
    NotFound(String),
    /// A folder that still holds something and was not asked to be deleted with its content.
    NotEmpty(String),
    Db(rusqlite::Error),
    Io(std::io::Error),
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Invalid(m) | Error::NotFound(m) | Error::NotEmpty(m) | Error::Other(m) => {
                f.write_str(m)
            }
            Error::Db(e) => write!(f, "The library database reported: {e}"),
            Error::Io(e) => write!(f, "The library folder could not be used: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Db(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<zip::result::ZipError> for Error {
    fn from(e: zip::result::ZipError) -> Self {
        Error::Other(format!("zip: {e}"))
    }
}
