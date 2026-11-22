//! JSON text exactly as ASP.NET's `Results.Json(.., JsonSerializerDefaults.Web)` writes it: compact, and strings escaped by
//! `JavaScriptEncoder.Default` (everything outside printable ASCII becomes `\uXXXX`, plus the HTML-sensitive characters
//! `"` `&` `'` `+` `<` `>` and the back quote).

use std::io;

use serde::Serialize;
use serde_json::ser::{CharEscape, Formatter};

struct WebFormatter;

fn write_u(w: &mut (impl ?Sized + io::Write), unit: u16) -> io::Result<()> {
    write!(w, "\\u{unit:04X}")
}

impl Formatter for WebFormatter {
    fn write_string_fragment<W: ?Sized + io::Write>(
        &mut self,
        w: &mut W,
        fragment: &str,
    ) -> io::Result<()> {
        let mut plain = String::new();
        for c in fragment.chars() {
            let cp = c as u32;
            let escape =
                !(0x20..0x7F).contains(&cp) || matches!(c, '&' | '\'' | '+' | '<' | '>' | '`');
            if escape {
                if !plain.is_empty() {
                    w.write_all(plain.as_bytes())?;
                    plain.clear();
                }
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) {
                    write_u(w, *u)?;
                }
            } else {
                plain.push(c);
            }
        }
        w.write_all(plain.as_bytes())
    }

    fn write_char_escape<W: ?Sized + io::Write>(
        &mut self,
        w: &mut W,
        ce: CharEscape,
    ) -> io::Result<()> {
        match ce {
            CharEscape::Quote => w.write_all(b"\\u0022"),
            CharEscape::ReverseSolidus => w.write_all(b"\\\\"),
            CharEscape::Solidus => w.write_all(b"/"),
            CharEscape::Backspace => w.write_all(b"\\b"),
            CharEscape::FormFeed => w.write_all(b"\\f"),
            CharEscape::LineFeed => w.write_all(b"\\n"),
            CharEscape::CarriageReturn => w.write_all(b"\\r"),
            CharEscape::Tab => w.write_all(b"\\t"),
            CharEscape::AsciiControl(b) => write_u(w, b as u16),
        }
    }
}

/// Serialises like System.Text.Json with web defaults (property names come from the serde attributes).
pub fn to_string<T: Serialize + ?Sized>(v: &T) -> String {
    let mut out = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(&mut out, WebFormatter);
    v.serialize(&mut ser).expect("serialisable");
    String::from_utf8(out).expect("utf-8")
}
