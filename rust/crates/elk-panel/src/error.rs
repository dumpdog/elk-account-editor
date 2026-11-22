//! Error type of the sync and live code. It carries either a panel-protocol error (`elk-core`), an account-file error (`elk-account`) or an
//! "unhandled" error (a .NET exception the C# routes do not catch, which ASP.NET answers with an empty 500).

use elk_core::ElkError;
use std::fmt;

/// The .NET exception class an error stands for; the route catch lists in `Program.cs` are written in these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `TimeoutException`.
    Timeout,
    /// `IOException` / `SocketException` / `ObjectDisposedException` (a broken link).
    Io,
    /// `UnauthorizedAccessException`.
    Unauthorized,
    /// `ArgumentException` (itself, not the out-of-range subclass).
    Argument,
    /// `ArgumentOutOfRangeException` (a subclass of `ArgumentException`).
    OutOfRange,
    /// `InvalidDataException`.
    InvalidData,
    /// `InvalidOperationException`.
    InvalidOperation,
    /// `FormatException`.
    Format,
    /// `JsonException`.
    Json,
    /// Anything the C# code does not catch.
    Other,
}

impl Kind {
    /// True when a C# `catch (ArgumentException)` would take it (`ArgumentOutOfRangeException` derives from it).
    pub fn is_argument(self) -> bool {
        matches!(self, Kind::Argument | Kind::OutOfRange)
    }
}

/// Error of a sync, pull, backup or live operation.
#[derive(Debug)]
pub enum PanelError {
    /// A panel / protocol error.
    Elk(ElkError),
    /// An account-file or model error.
    Account(elk_account::Error),
    /// An exception the C# routes leave unhandled (HTTP 500).
    Other(String),
}

impl PanelError {
    /// The .NET exception class.
    pub fn kind(&self) -> Kind {
        match self {
            PanelError::Elk(e) => match e {
                ElkError::Io(_) => Kind::Io,
                ElkError::Timeout(_) => Kind::Timeout,
                ElkError::InvalidData(_) => Kind::InvalidData,
                ElkError::Unauthorized(_) => Kind::Unauthorized,
                ElkError::InvalidOperation(_) => Kind::InvalidOperation,
                ElkError::Argument(_) => Kind::Argument,
                ElkError::OutOfRange(_) => Kind::OutOfRange,
            },
            PanelError::Account(e) => match e {
                elk_account::Error::Format(_) => Kind::Format,
                elk_account::Error::InvalidData(_) => Kind::InvalidData,
                elk_account::Error::OutOfRange(_) => Kind::OutOfRange,
                elk_account::Error::InvalidOperation(_) => Kind::InvalidOperation,
                elk_account::Error::Argument(_) => Kind::Argument,
            },
            PanelError::Other(_) => Kind::Other,
        }
    }

    /// True when the C# code treats it as a broken link (`IsLinkError`): the session is closed and `Error` says why.
    pub fn is_link_error(&self) -> bool {
        matches!(self.kind(), Kind::Timeout | Kind::Io | Kind::InvalidData) && !matches!(self, PanelError::Account(_))
    }

    /// `Exception.Message`.
    pub fn message(&self) -> String {
        self.to_string()
    }

    /// An exception the C# code does not catch.
    pub fn other(m: impl Into<String>) -> Self {
        PanelError::Other(m.into())
    }
}

impl fmt::Display for PanelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PanelError::Elk(e) => write!(f, "{e}"),
            PanelError::Account(e) => write!(f, "{e}"),
            PanelError::Other(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for PanelError {}

impl From<ElkError> for PanelError {
    fn from(e: ElkError) -> Self {
        PanelError::Elk(e)
    }
}

impl From<elk_account::Error> for PanelError {
    fn from(e: elk_account::Error) -> Self {
        PanelError::Account(e)
    }
}

impl From<std::io::Error> for PanelError {
    fn from(e: std::io::Error) -> Self {
        PanelError::Elk(ElkError::Io(e))
    }
}

/// `Result` with [`PanelError`].
pub type Result<T> = std::result::Result<T, PanelError>;
