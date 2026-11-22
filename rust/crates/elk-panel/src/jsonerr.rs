//! `System.Text.Json` error texts for the request bodies: the syntax errors seen in practice (empty body, bad start of a value, trailing comma, text after
//! the value, unterminated string or container) and the member spans needed for "could not be converted" messages with their byte position.
//! Other syntax errors fall back to the text of `serde_json` (the wording then differs from .NET).

/// A member of the top-level object: its name as written, and the byte range of its value.
#[derive(Debug, Clone)]
pub struct Span {
    pub key: String,
    pub start: usize,
    pub end: usize,
}

struct Scan<'a> {
    b: &'a [u8],
    i: usize,
}

type Fail = (String, usize, String); // message, byte position, path

fn path_text(path: &[String]) -> String {
    format!("${}", path.concat())
}

fn msg(m: &str, pos: usize, path: &[String]) -> Fail {
    (m.to_string(), pos, path_text(path))
}

impl Scan<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\r' | b'\n') {
            self.i += 1;
        }
    }

    fn string(&mut self, path: &[String]) -> std::result::Result<String, Fail> {
        // at the opening quote
        self.i += 1;
        let mut raw: Vec<u8> = Vec::new();
        while self.i < self.b.len() {
            match self.b[self.i] {
                b'"' => {
                    self.i += 1;
                    return Ok(String::from_utf8_lossy(&raw).into_owned());
                }
                b'\\' => {
                    self.i += 1;
                    if self.i < self.b.len() {
                        raw.push(self.b[self.i]);
                        self.i += 1;
                    }
                }
                c => {
                    raw.push(c);
                    self.i += 1;
                }
            }
        }
        Err(msg("Expected end of string, but instead reached end of data.", self.b.len(), path))
    }

    fn value(&mut self, path: &mut Vec<String>, spans: Option<&mut Vec<Span>>) -> std::result::Result<(), Fail> {
        self.ws();
        let Some(&c) = self.b.get(self.i) else {
            let mut f = Self::open_container();
            f.1 = self.b.len();
            return Err(f);
        };
        match c {
            b'{' => self.object(path, spans),
            b'[' => self.array(path),
            b'"' => self.string(path).map(|_| ()),
            b'-' | b'0'..=b'9' => {
                while self.i < self.b.len() && matches!(self.b[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                    self.i += 1;
                }
                Ok(())
            }
            b't' | b'f' | b'n' => {
                for lit in ["true", "false", "null"] {
                    if self.b[self.i..].starts_with(lit.as_bytes()) {
                        self.i += lit.len();
                        return Ok(());
                    }
                }
                Err(msg(&format!("'{}' is an invalid start of a value.", c as char), self.i, path))
            }
            _ => Err(msg(&format!("'{}' is an invalid start of a value.", c as char), self.i, path)),
        }
    }

    fn open_container() -> Fail {
        ("Expected depth to be zero at the end of the JSON payload. There is an open JSON object or array that should be closed.".into(), 0, "$".into())
    }

    fn object(&mut self, path: &mut Vec<String>, mut spans: Option<&mut Vec<Span>>) -> std::result::Result<(), Fail> {
        self.i += 1;
        let mut first = true;
        loop {
            self.ws();
            let Some(&c) = self.b.get(self.i) else {
                let mut f = Self::open_container();
                f.1 = self.b.len();
                return Err(f);
            };
            if c == b'}' {
                if !first {
                    return Err(msg("The JSON object contains a trailing comma at the end which is not supported in this mode. Change the reader options.", self.i, path));
                }
                self.i += 1;
                return Ok(());
            }
            if c != b'"' {
                return Err(msg(&format!("'{}' is an invalid start of a property name. Expected a '\"'.", c as char), self.i, path));
            }
            let key = self.string(path)?;
            self.ws();
            if self.b.get(self.i) != Some(&b':') {
                return match self.b.get(self.i) {
                    None => {
                        let mut f = Self::open_container();
                        f.1 = self.b.len();
                        Err(f)
                    }
                    Some(c) => Err(msg(&format!("'{}' is invalid after a property name. Expected a ':'.", *c as char), self.i, path)),
                };
            }
            self.i += 1;
            self.ws();
            let start = self.i;
            path.push(format!(".{key}"));
            self.value(path, None)?;
            path.pop();
            if let Some(sp) = spans.as_deref_mut() {
                sp.push(Span { key, start, end: self.i });
            }
            self.ws();
            match self.b.get(self.i) {
                Some(b',') => {
                    self.i += 1;
                    first = false; // a '}' right after the comma is the trailing-comma case, handled at the top of the loop
                }
                Some(b'}') => {
                    self.i += 1;
                    return Ok(());
                }
                None => {
                    let mut f = Self::open_container();
                    f.1 = self.b.len();
                    return Err(f);
                }
                Some(c) => return Err(msg(&format!("'{}' is invalid after a property value. Expected either ',' or '}}'.", *c as char), self.i, path)),
            }
        }
    }

    fn array(&mut self, path: &mut Vec<String>) -> std::result::Result<(), Fail> {
        self.i += 1;
        let mut n = 0;
        loop {
            self.ws();
            match self.b.get(self.i) {
                None => {
                    let mut f = Self::open_container();
                    f.1 = self.b.len();
                    return Err(f);
                }
                Some(b']') => {
                    if n > 0 {
                        return Err(msg("The JSON array contains a trailing comma at the end which is not supported in this mode. Change the reader options.", self.i, path));
                    }
                    self.i += 1;
                    return Ok(());
                }
                _ => {}
            }
            path.push(format!("[{n}]"));
            self.value(path, None)?;
            path.pop();
            self.ws();
            match self.b.get(self.i) {
                Some(b',') => {
                    self.i += 1;
                    n += 1;
                }
                Some(b']') => {
                    self.i += 1;
                    return Ok(());
                }
                None => {
                    let mut f = Self::open_container();
                    f.1 = self.b.len();
                    return Err(f);
                }
                Some(c) => return Err(msg(&format!("'{}' is invalid after an array value. Expected either ',' or ']'.", *c as char), self.i, path)),
            }
        }
    }
}

fn format_fail((m, pos, path): Fail) -> String {
    format!("{m} Path: {path} | LineNumber: 0 | BytePositionInLine: {pos}.")
}

/// Scans a JSON text. `Ok` carries the spans of the top-level object's members (empty when the root is not an object) and the byte offset of the first
/// byte of the root value; `Err` is the `JsonException` message.
pub fn scan(b: &[u8]) -> std::result::Result<(Vec<Span>, usize, usize), String> {
    let b = b.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(b);
    let mut s = Scan { b, i: 0 };
    s.ws();
    if s.i >= b.len() {
        return Err("The input does not contain any JSON tokens. Expected the input to start with a valid JSON token, when isFinalBlock is true. Path: $ | LineNumber: 0 | BytePositionInLine: 0.".into());
    }
    let root_start = s.i;
    let mut spans = Vec::new();
    let mut path: Vec<String> = Vec::new();
    s.value(&mut path, Some(&mut spans)).map_err(|mut f| {
        if f.2 == "$" && f.0.starts_with("Expected depth") {
            f.1 = b.len();
        }
        format_fail(f)
    })?;
    let root_end = s.i;
    s.ws();
    if s.i < b.len() {
        return Err(format_fail((format!("'{}' is invalid after a single JSON value. Expected end of data.", b[s.i] as char), s.i, "$".into())));
    }
    Ok((spans, root_start, root_end))
}
