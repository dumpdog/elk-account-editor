//! Error type. The variants mirror the .NET exceptions the C# code throws; `message()` is the text the server returns.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// `FormatException` (also used for number parse failures).
    Format(String),
    /// `InvalidDataException`.
    InvalidData(String),
    /// `ArgumentOutOfRangeException`.
    OutOfRange(String),
    /// `InvalidOperationException`.
    InvalidOperation(String),
    /// `ArgumentException`.
    Argument(String),
}

impl Error {
    pub fn message(&self) -> &str {
        match self {
            Error::Format(m)
            | Error::InvalidData(m)
            | Error::OutOfRange(m)
            | Error::InvalidOperation(m)
            | Error::Argument(m) => m,
        }
    }
    pub(crate) fn format(m: impl Into<String>) -> Self {
        Error::Format(m.into())
    }
    pub(crate) fn invalid_data(m: impl Into<String>) -> Self {
        Error::InvalidData(m.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// .NET `FormatException` text for a failed integer parse (.NET 8).
pub(crate) fn bad_number(s: &str) -> Error {
    Error::Format(format!(
        "The input string '{s}' was not in a correct format."
    ))
}

/// `int.TryParse(s, NumberStyles.None, ..)` and `<= max`: digits only, no sign or spaces.
pub(crate) fn is_num(s: &str, max: i32) -> Option<i32> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let v: i32 = s.parse().ok()?;
    (v <= max).then_some(v)
}

/// `int.Parse` / `int.TryParse` with the default `NumberStyles.Integer`: optional surrounding white space and one sign.
pub(crate) fn parse_int_loose(s: &str) -> Option<i32> {
    let t = s.trim();
    let digits = t.strip_prefix(['+', '-']).unwrap_or(t);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    t.parse::<i32>().ok()
}

/// `byte.Parse(s, InvariantCulture)`.
pub(crate) fn parse_byte(s: &str) -> Result<u8> {
    let t = s.trim();
    let digits = t.strip_prefix(['+', '-']).unwrap_or(t);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad_number(s));
    }
    match t.parse::<i64>() {
        Ok(v) if (0..=255).contains(&v) => Ok(v as u8),
        _ => Err(Error::Format(
            "Value was either too large or too small for an unsigned byte.".into(),
        )),
    }
}
