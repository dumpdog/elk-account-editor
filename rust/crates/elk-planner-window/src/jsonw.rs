//! Minimal JSON output helpers that reproduce System.Text.Json's default encoder, so output is byte-identical to the C# server.

use std::fmt::Write;

/// Appends `s` as a JSON string literal the way `JavaScriptEncoder.Default` does: everything outside
/// printable ASCII is `\uXXXX`, and the HTML-sensitive characters `" & ' + < > \`` are escaped too.
pub fn push_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '"' | '&' | '\'' | '+' | '<' | '>' | '`' => {
                let _ = write!(out, "\\u{:04X}", c as u32);
            }
            ' '..='~' => out.push(c),
            _ => {
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) {
                    let _ = write!(out, "\\u{:04X}", u);
                }
            }
        }
    }
    out.push('"');
}

/// Appends a double the way `Utf8JsonWriter.WriteNumberValue(double)` does (shortest round trip,
/// `E+XX` notation from 1e17 up and below 1e-4 (probed against the C# server)).
pub fn push_f64(out: &mut String, v: f64) {
    if v == 0.0 {
        out.push_str(if v.is_sign_negative() { "-0" } else { "0" });
        return;
    }
    let sci = format!("{:e}", v); // e.g. "-1.2345e20"
    let (mant, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    if (-4..17).contains(&exp) {
        let _ = write!(out, "{}", v);
    } else {
        let sign = if exp < 0 { '-' } else { '+' };
        let _ = write!(out, "{}E{}{:02}", mant, sign, exp.abs());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_match_the_default_encoder() {
        let mut s = String::new();
        push_str(&mut s, "a \"q\" <b> & 'c' + `d` \\ \n é → 😀");
        assert_eq!(
            s,
            r#""a \u0022q\u0022 \u003Cb\u003E \u0026 \u0027c\u0027 \u002B \u0060d\u0060 \\ \n \u00E9 \u2192 \uD83D\uDE00""#
        );
    }

    #[test]
    fn doubles_match_the_default_writer() {
        let f = |v: f64| {
            let mut s = String::new();
            push_f64(&mut s, v);
            s
        };
        assert_eq!(f(150.0), "150");
        assert_eq!(f(1.5), "1.5");
        assert_eq!(f(1e15), "1000000000000000");
        assert_eq!(f(1e16), "10000000000000000");
        assert_eq!(f(1e17), "1E+17");
        assert_eq!(f(1.2345678901234568e17), "1.2345678901234568E+17");
        assert_eq!(f(0.000099), "9.9E-05");
        assert_eq!(f(1.5e-7), "1.5E-07");
        assert_eq!(f(0.0001), "0.0001");
        assert_eq!(f(-3.0), "-3");
    }
}
