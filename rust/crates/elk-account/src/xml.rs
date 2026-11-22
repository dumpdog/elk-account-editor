//! A small mutable XML tree, enough for spreadsheetml parts. It follows what `XDocument.Load` / `Save(DisableFormatting)`
//! do for these files: insignificant white space is dropped, namespace declarations stay in the attribute list in place,
//! empty elements without an end tag are written as `<a />`.

use crate::error::{Error, Result};

pub const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";
const XMLNS_NS: &str = "http://www.w3.org/2000/xmlns/";

#[derive(Debug, Clone)]
pub struct Attr {
    pub prefix: String,
    pub local: String,
    /// Resolved namespace (empty for none; the xmlns declarations use the xmlns namespace).
    pub ns: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub enum Node {
    Elem(Element),
    Text(String),
}

#[derive(Debug, Clone)]
pub struct Element {
    pub prefix: String,
    pub local: String,
    pub ns: String,
    pub attrs: Vec<Attr>,
    pub children: Vec<Node>,
    /// Parsed from `<a></a>` (written back the same way); false means `<a />` when it has no children.
    pub explicit_end: bool,
}

impl Element {
    /// A new element in namespace `ns` that is the document's default namespace (no prefix).
    pub fn new(ns: &str, local: &str) -> Element {
        Element {
            prefix: String::new(),
            local: local.into(),
            ns: ns.into(),
            attrs: Vec::new(),
            children: Vec::new(),
            explicit_end: false,
        }
    }

    pub fn is(&self, ns: &str, local: &str) -> bool {
        self.local == local && self.ns == ns
    }

    /// `XElement.Attribute(name)` for an attribute without a namespace.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| a.ns.is_empty() && a.prefix.is_empty() && a.local == name)
            .map(|a| a.value.as_str())
    }

    /// An attribute in a namespace (e.g. `r:id`).
    pub fn attr_ns(&self, ns: &str, local: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| a.ns == ns && a.local == local)
            .map(|a| a.value.as_str())
    }

    /// `SetAttributeValue`: replaces in place, or appends.
    pub fn set_attr(&mut self, name: &str, value: &str) {
        if let Some(a) = self
            .attrs
            .iter_mut()
            .find(|a| a.ns.is_empty() && a.prefix.is_empty() && a.local == name)
        {
            a.value = value.into();
        } else {
            self.attrs.push(Attr {
                prefix: String::new(),
                local: name.into(),
                ns: String::new(),
                value: value.into(),
            });
        }
    }

    pub fn set_attr_ns(&mut self, prefix: &str, ns: &str, local: &str, value: &str) {
        self.attrs.push(Attr {
            prefix: prefix.into(),
            local: local.into(),
            ns: ns.into(),
            value: value.into(),
        });
    }

    pub fn remove_attr(&mut self, name: &str) {
        self.attrs
            .retain(|a| !(a.ns.is_empty() && a.prefix.is_empty() && a.local == name));
    }

    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children
            .iter()
            .filter_map(|n| if let Node::Elem(e) = n { Some(e) } else { None })
    }

    pub fn elements_mut(&mut self) -> impl Iterator<Item = &mut Element> {
        self.children
            .iter_mut()
            .filter_map(|n| if let Node::Elem(e) = n { Some(e) } else { None })
    }

    pub fn child(&self, ns: &str, local: &str) -> Option<&Element> {
        self.elements().find(|e| e.is(ns, local))
    }

    pub fn child_mut(&mut self, ns: &str, local: &str) -> Option<&mut Element> {
        self.elements_mut().find(|e| e.is(ns, local))
    }

    pub fn add(&mut self, e: Element) {
        self.children.push(Node::Elem(e));
    }

    /// `XElement.Value`: all descendant text.
    pub fn value(&self) -> String {
        let mut s = String::new();
        self.collect_text(&mut s);
        s
    }

    fn collect_text(&self, out: &mut String) {
        for n in &self.children {
            match n {
                Node::Text(t) => out.push_str(t),
                Node::Elem(e) => e.collect_text(out),
            }
        }
    }

    /// Replaces the content with one text node (`XElement.Value = ..`).
    pub fn set_value(&mut self, v: &str) {
        self.children.clear();
        if !v.is_empty() {
            self.children.push(Node::Text(v.into()));
        }
        self.explicit_end = true;
    }

    /// `Descendants(name)` in document order.
    pub fn descendants<'a>(&'a self, ns: &str, local: &str, out: &mut Vec<&'a Element>) {
        for e in self.elements() {
            if e.is(ns, local) {
                out.push(e);
            }
            e.descendants(ns, local, out);
        }
    }

    /// The first descendant in document order, mutable.
    pub fn first_descendant_mut(&mut self, ns: &str, local: &str) -> Option<&mut Element> {
        for e in self.elements_mut() {
            if e.is(ns, local) {
                return Some(e);
            }
            if let Some(f) = e.first_descendant_mut(ns, local) {
                return Some(f);
            }
        }
        None
    }

    /// Removes every descendant element matching, anywhere below.
    pub fn remove_descendants(&mut self, ns: &str, local: &str) {
        let before = self.children.len();
        self.children
            .retain(|n| !matches!(n, Node::Elem(e) if e.is(ns, local)));
        if before > 0 && self.children.is_empty() {
            self.explicit_end = false;
        }
        for e in self.elements_mut() {
            e.remove_descendants(ns, local);
        }
    }

    pub fn for_each_descendant_mut(
        &mut self,
        ns: &str,
        local: &str,
        f: &mut dyn FnMut(&mut Element),
    ) {
        for e in self.elements_mut() {
            if e.is(ns, local) {
                f(e);
            }
            e.for_each_descendant_mut(ns, local, f);
        }
    }

    fn write(&self, out: &mut String) {
        out.push('<');
        self.write_name(out);
        for a in &self.attrs {
            out.push(' ');
            if !a.prefix.is_empty() {
                out.push_str(&a.prefix);
                out.push(':');
            }
            out.push_str(&a.local);
            out.push_str("=\"");
            escape_attr(&a.value, out);
            out.push('"');
        }
        if self.children.is_empty() && !self.explicit_end {
            out.push_str(" />");
            return;
        }
        out.push('>');
        for n in &self.children {
            match n {
                Node::Text(t) => escape_text(t, out),
                Node::Elem(e) => e.write(out),
            }
        }
        out.push_str("</");
        self.write_name(out);
        out.push('>');
    }

    fn write_name(&self, out: &mut String) {
        if !self.prefix.is_empty() {
            out.push_str(&self.prefix);
            out.push(':');
        }
        out.push_str(&self.local);
    }
}

fn escape_text(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("&#xD;"),
            c => out.push(c),
        }
    }
}

fn escape_attr(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("&#xA;"),
            '\r' => out.push_str("&#xD;"),
            '\t' => out.push_str("&#x9;"),
            c => out.push(c),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Document {
    pub root: Element,
}

impl Document {
    /// `XDocument.Save(.., DisableFormatting)` with the declaration `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>`.
    pub fn to_string_with_decl(&self) -> String {
        let mut s = String::from("<?xml version=\"1.0\" encoding=\"utf-8\" standalone=\"yes\"?>");
        self.root.write(&mut s);
        s
    }

    pub fn parse(text: &str) -> Result<Document> {
        let mut p = Parser {
            s: text.as_bytes(),
            text,
            i: 0,
        };
        p.run()
    }
}

struct Parser<'a> {
    s: &'a [u8],
    text: &'a str,
    i: usize,
}

struct Scope {
    decls: Vec<(String, String)>,
}

fn err(m: &str) -> Error {
    Error::InvalidData(format!("XML error: {m}"))
}

impl Parser<'_> {
    fn starts(&self, p: &str) -> bool {
        self.s[self.i..].starts_with(p.as_bytes())
    }

    fn skip_until(&mut self, end: &str) -> Result<()> {
        match self.text[self.i..].find(end) {
            Some(k) => {
                self.i += k + end.len();
                Ok(())
            }
            None => Err(err("unexpected end of document")),
        }
    }

    fn run(&mut self) -> Result<Document> {
        if self.starts("\u{feff}") {
            self.i += 3;
        }
        // prolog
        loop {
            self.skip_ws();
            if self.starts("<?") {
                self.skip_until("?>")?;
            } else if self.starts("<!--") {
                self.skip_until("-->")?;
            } else if self.starts("<!DOCTYPE") {
                self.skip_until(">")?;
            } else {
                break;
            }
        }
        let mut scopes: Vec<Scope> = vec![Scope {
            decls: vec![("xml".into(), XML_NS.into())],
        }];
        let root = self.element(&mut scopes, false)?;
        Ok(Document { root })
    }

    fn skip_ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn name(&mut self) -> Result<String> {
        let st = self.i;
        while self.i < self.s.len() {
            let b = self.s[self.i];
            if b.is_ascii_whitespace() || matches!(b, b'=' | b'>' | b'/' | b'<' | b'"' | b'\'') {
                break;
            }
            self.i += 1;
        }
        if st == self.i {
            return Err(err("name expected"));
        }
        Ok(self.text[st..self.i].to_string())
    }

    fn resolve(scopes: &[Scope], prefix: &str) -> String {
        for sc in scopes.iter().rev() {
            for (p, u) in sc.decls.iter().rev() {
                if p == prefix {
                    return u.clone();
                }
            }
        }
        String::new()
    }

    fn element(&mut self, scopes: &mut Vec<Scope>, preserve: bool) -> Result<Element> {
        if self.i >= self.s.len() || self.s[self.i] != b'<' {
            return Err(err("element expected"));
        }
        self.i += 1;
        let qname = self.name()?;
        let (prefix, local) = split_q(&qname);
        let mut raw_attrs: Vec<(String, String, String)> = Vec::new(); // prefix, local, value
        let mut self_closed = false;
        loop {
            self.skip_ws();
            if self.i >= self.s.len() {
                return Err(err("unexpected end of document"));
            }
            match self.s[self.i] {
                b'/' => {
                    if !self.starts("/>") {
                        return Err(err("bad tag"));
                    }
                    self.i += 2;
                    self_closed = true;
                    break;
                }
                b'>' => {
                    self.i += 1;
                    break;
                }
                _ => {
                    let an = self.name()?;
                    self.skip_ws();
                    if self.i >= self.s.len() || self.s[self.i] != b'=' {
                        return Err(err("attribute value expected"));
                    }
                    self.i += 1;
                    self.skip_ws();
                    let q = *self.s.get(self.i).ok_or_else(|| err("unexpected end"))?;
                    if q != b'"' && q != b'\'' {
                        return Err(err("quote expected"));
                    }
                    self.i += 1;
                    let st = self.i;
                    while self.i < self.s.len() && self.s[self.i] != q {
                        self.i += 1;
                    }
                    if self.i >= self.s.len() {
                        return Err(err("unterminated attribute"));
                    }
                    let raw = &self.text[st..self.i];
                    self.i += 1;
                    let (ap, al) = split_q(&an);
                    // attribute value normalisation: literal white space becomes a space
                    let val = decode_entities(raw)?.replace(['\t', '\n', '\r'], " ");
                    raw_attrs.push((ap, al, val));
                }
            }
        }
        let mut sc = Scope { decls: Vec::new() };
        for (ap, al, v) in &raw_attrs {
            if ap == "xmlns" {
                sc.decls.push((al.clone(), v.clone()));
            } else if ap.is_empty() && al == "xmlns" {
                sc.decls.push((String::new(), v.clone()));
            }
        }
        scopes.push(sc);
        let ns = Self::resolve(scopes, &prefix);
        let mut el = Element {
            prefix,
            local,
            ns,
            attrs: Vec::new(),
            children: Vec::new(),
            explicit_end: false,
        };
        let mut preserve = preserve;
        for (ap, al, v) in raw_attrs {
            let ans = if ap == "xmlns" || (ap.is_empty() && al == "xmlns") {
                XMLNS_NS.to_string()
            } else if ap.is_empty() {
                String::new()
            } else {
                Self::resolve(scopes, &ap)
            };
            if ans == XML_NS && al == "space" {
                preserve = v == "preserve";
            }
            el.attrs.push(Attr {
                prefix: ap,
                local: al,
                ns: ans,
                value: v,
            });
        }
        if self_closed {
            scopes.pop();
            return Ok(el);
        }
        el.explicit_end = true;
        let mut text = String::new();
        loop {
            if self.i >= self.s.len() {
                return Err(err("unexpected end of document"));
            }
            if self.starts("</") {
                self.flush_text(&mut text, &mut el, preserve);
                self.i += 2;
                let en = self.name()?;
                self.skip_ws();
                if self.i >= self.s.len() || self.s[self.i] != b'>' || en != qname {
                    return Err(err("mismatched end tag"));
                }
                self.i += 1;
                break;
            } else if self.starts("<!--") {
                self.skip_until("-->")?;
            } else if self.starts("<![CDATA[") {
                self.i += 9;
                let st = self.i;
                self.skip_until("]]>")?;
                text.push_str(&self.text[st..self.i - 3]);
            } else if self.starts("<?") {
                self.skip_until("?>")?;
            } else if self.s[self.i] == b'<' {
                self.flush_text(&mut text, &mut el, preserve);
                let child = self.element(scopes, preserve)?;
                el.children.push(Node::Elem(child));
            } else {
                let st = self.i;
                while self.i < self.s.len() && self.s[self.i] != b'<' {
                    self.i += 1;
                }
                text.push_str(
                    &decode_entities(&self.text[st..self.i])?
                        .replace("\r\n", "\n")
                        .replace('\r', "\n"),
                );
            }
        }
        scopes.pop();
        Ok(el)
    }

    fn flush_text(&self, text: &mut String, el: &mut Element, preserve: bool) {
        if text.is_empty() {
            return;
        }
        if preserve || text.chars().any(|c| !matches!(c, ' ' | '\t' | '\n' | '\r')) {
            el.children.push(Node::Text(std::mem::take(text)));
        } else {
            text.clear();
        }
    }
}

fn split_q(q: &str) -> (String, String) {
    match q.split_once(':') {
        Some((p, l)) => (p.to_string(), l.to_string()),
        None => (String::new(), q.to_string()),
    }
}

fn decode_entities(s: &str) -> Result<String> {
    if !s.contains('&') {
        return Ok(s.to_string());
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(k) = rest.find('&') {
        out.push_str(&rest[..k]);
        rest = &rest[k + 1..];
        let end = rest.find(';').ok_or_else(|| err("bad entity"))?;
        let ent = &rest[..end];
        match ent {
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "amp" => out.push('&'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            _ => {
                let code =
                    if let Some(h) = ent.strip_prefix("#x").or_else(|| ent.strip_prefix("#X")) {
                        u32::from_str_radix(h, 16).ok()
                    } else if let Some(d) = ent.strip_prefix('#') {
                        d.parse::<u32>().ok()
                    } else {
                        None
                    };
                out.push(
                    code.and_then(char::from_u32)
                        .ok_or_else(|| err("bad entity"))?,
                );
            }
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}
