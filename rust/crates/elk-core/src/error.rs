//! Error type shared by every module. The variants map to the .NET exceptions the C# code throws.

use std::fmt;
use std::io;

/// Error of any panel operation.
#[derive(Debug)]
pub enum ElkError {
    /// `IOException` (socket errors, connection closed by the panel, TLS handshake failure).
    Io(io::Error),
    /// `TimeoutException`: no reply within `ElkSession::timeout`.
    Timeout(String),
    /// `InvalidDataException`: a reply or file that is malformed.
    InvalidData(String),
    /// `UnauthorizedAccessException`: the panel rejected the RP access code.
    Unauthorized(String),
    /// `InvalidOperationException` (for example arming an area that is not ready).
    InvalidOperation(String),
    /// `ArgumentException`.
    Argument(String),
    /// `ArgumentOutOfRangeException`.
    OutOfRange(String),
}

/// `Result` with [`ElkError`].
pub type Result<T> = std::result::Result<T, ElkError>;

impl ElkError {
    /// True for [`ElkError::Timeout`] (C# `catch (TimeoutException)`).
    pub fn is_timeout(&self) -> bool {
        matches!(self, ElkError::Timeout(_))
    }
    /// True for [`ElkError::Io`] (C# `catch (IOException)`).
    pub fn is_io(&self) -> bool {
        matches!(self, ElkError::Io(_))
    }
}

impl fmt::Display for ElkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ElkError::Io(e) => write!(f, "{e}"),
            ElkError::Timeout(m) | ElkError::InvalidData(m) | ElkError::Unauthorized(m) | ElkError::InvalidOperation(m) | ElkError::Argument(m) | ElkError::OutOfRange(m) => {
                f.write_str(m)
            }
        }
    }
}

impl std::error::Error for ElkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ElkError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for ElkError {
    fn from(e: io::Error) -> Self {
        ElkError::Io(e)
    }
}

/// `Convert.ToHexString`: upper-case hex.
pub fn hex_upper(data: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(data.len() * 2);
    for b in data {
        let _ = write!(s, "{b:02X}");
    }
    s
}

/// `Convert.FromHexString`: even length, hex digits of either case.
pub fn hex_decode(text: &str) -> Result<Vec<u8>> {
    let b = text.as_bytes();
    if !b.len().is_multiple_of(2) {
        return Err(ElkError::InvalidData("The input is not a valid hex string (odd length).".into()));
    }
    let digit = |c: u8| -> Result<u8> { (c as char).to_digit(16).map(|d| d as u8).ok_or_else(|| ElkError::InvalidData("The input is not a valid hex string.".into())) };
    b.chunks(2).map(|p| Ok((digit(p[0])? << 4) | digit(p[1])?)).collect()
}
