//! Reads a zone list from an Excel workbook (.xlsx) or a CSV/TSV file and writes one back out.
//!
//! Columns are found by their header text (Zone / Number / #, Name / Description, Definition / Function, Type / Wiring,
//! Area / Partition), in any order. With no header row the columns are Zone, Name, Definition, Type, Area, the same as the
//! paste box. A sheet that has a Name column but no Zone column is numbered from zone 1 in row order. Definition and Type may
//! be given as the text ElkRP shows ("Burglar Entry/Exit 1") or as the number.

use std::collections::HashMap;
use std::io::{Cursor, Read, Write};

use crate::account_model::{ModelMeta, ZoneDto};
use crate::error::{parse_int_loose, Error, Result};
use crate::xml::{Document, Element, Node};

const NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// One zone row from a spreadsheet. A None field means the sheet had no such column (or the cell was empty), so the zone keeps what it has.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ZoneSheetRow {
    pub n: i32,
    pub name: Option<String>,
    pub definition: Option<i32>,
    #[serde(rename = "type")]
    pub type_: Option<i32>,
    pub area: Option<i32>,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ZoneSheetResult {
    pub rows: Vec<ZoneSheetRow>,
    pub columns: Vec<String>,
    pub warnings: Vec<String>,
}

pub struct ZoneSheet;

fn is_letter_or_digit(c: char) -> bool {
    c.is_alphanumeric()
}

fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| is_letter_or_digit(*c))
        .flat_map(char::to_lowercase)
        .collect()
}

impl ZoneSheet {
    pub const MAX_ZONE: i32 = 208;

    // ---- reading ----------------------------------------------------------------------------------------------------

    pub fn read(data: &[u8], file_name: &str) -> Result<ZoneSheetResult> {
        let zip = data.len() > 3 && data[0] == b'P' && data[1] == b'K';
        let table = if zip {
            Self::read_xlsx(data)?
        } else if file_name.to_lowercase().ends_with(".xls")
            && data.len() > 3
            && data[0] == 0xD0
            && data[1] == 0xCF
        {
            return Err(Error::invalid_data(
                "This is an old Excel (.xls) file. Open it in Excel and use Save As > Excel Workbook (.xlsx) or CSV.",
            ));
        } else {
            Self::read_delimited(&Self::decode_text(data))
        };
        Self::interpret(&table)
    }

    fn decode_text(d: &[u8]) -> String {
        if d.len() >= 3 && d[0] == 0xEF && d[1] == 0xBB && d[2] == 0xBF {
            return String::from_utf8_lossy(&d[3..]).into_owned();
        }
        if d.len() >= 2 && d[0] == 0xFF && d[1] == 0xFE {
            // Excel "Unicode Text"
            let units: Vec<u16> = d[2..]
                .chunks(2)
                .map(|c| u16::from_le_bytes([c[0], *c.get(1).unwrap_or(&0)]))
                .collect();
            return char::decode_utf16(units)
                .map(|r| r.unwrap_or('\u{FFFD}'))
                .collect();
        }
        match std::str::from_utf8(d) {
            Ok(s) => s.to_string(),
            Err(_) => crate::account_file::decode_latin1(d),
        }
    }

    /// Splits CSV or tab separated text. The separator is whichever of tab, semicolon or comma the first line has most of
    /// (tab wins ties); quotes work as in Excel.
    pub fn read_delimited(text: &str) -> Vec<Vec<String>> {
        let first: &str = match text.find(['\r', '\n']) {
            Some(nl) => &text[..nl],
            None => text,
        };
        let count = |c: char| first.chars().filter(|&x| x == c).count();
        let mut sep = ',';
        let mut best = count(',');
        if count(';') > best {
            sep = ';';
            best = count(';');
        }
        if count('\t') >= best.max(1) {
            sep = '\t';
        }
        let chars: Vec<char> = text.chars().collect();
        let mut rows: Vec<Vec<String>> = Vec::new();
        let mut row: Vec<String> = Vec::new();
        let mut sb = String::new();
        let (mut q, mut any) = (false, false);
        fn end_cell(row: &mut Vec<String>, sb: &mut String, any: &mut bool) {
            row.push(std::mem::take(sb));
            *any = true;
        }
        fn end_row(
            rows: &mut Vec<Vec<String>>,
            row: &mut Vec<String>,
            sb: &mut String,
            any: &mut bool,
        ) {
            if *any || !sb.is_empty() {
                end_cell(row, sb, any);
                rows.push(std::mem::take(row));
            }
            *row = Vec::new();
            *any = false;
        }
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if q {
                if c == '"' {
                    if i + 1 < chars.len() && chars[i + 1] == '"' {
                        sb.push('"');
                        i += 1;
                    } else {
                        q = false;
                    }
                } else {
                    sb.push(c);
                }
            } else if c == '"' && sb.is_empty() {
                q = true;
                any = true;
            } else if c == sep {
                end_cell(&mut row, &mut sb, &mut any);
            } else if c == '\r' || c == '\n' {
                if c == '\r' && i + 1 < chars.len() && chars[i + 1] == '\n' {
                    i += 1;
                }
                end_row(&mut rows, &mut row, &mut sb, &mut any);
            } else {
                sb.push(c);
            }
            i += 1;
        }
        end_row(&mut rows, &mut row, &mut sb, &mut any);
        rows
    }

    fn load(z: &mut zip::ZipArchive<Cursor<&[u8]>>, path: &str) -> Result<Document> {
        let mut e = z
            .by_name(path)
            .map_err(|_| Error::invalid_data(format!("The workbook is missing {path}.")))?;
        let mut buf = Vec::new();
        e.read_to_end(&mut buf)
            .map_err(|x| Error::invalid_data(x.to_string()))?;
        Document::parse(&String::from_utf8_lossy(&buf))
    }

    fn read_xlsx(data: &[u8]) -> Result<Vec<Vec<String>>> {
        let wrap = |e: Error| {
            Error::invalid_data(format!("Could not read this Excel file: {}", e.message()))
        };
        (|| -> Result<Vec<Vec<String>>> {
            let mut z = zip::ZipArchive::new(Cursor::new(data))
                .map_err(|_| Error::invalid_data("Central Directory corrupt."))?;
            let mut shared: Vec<String> = Vec::new();
            if z.by_name("xl/sharedStrings.xml").is_ok() {
                let doc = Self::load(&mut z, "xl/sharedStrings.xml")?;
                for si in doc.root.elements().filter(|e| e.is(NS, "si")) {
                    let mut s = String::new();
                    collect_t_skip_phonetic(si, &mut s);
                    shared.push(s);
                }
            }
            // the first worksheet, or one called "Zones" if there is
            let wb = Self::load(&mut z, "xl/workbook.xml")?;
            let rels_doc = Self::load(&mut z, "xl/_rels/workbook.xml.rels")?;
            let mut rels: HashMap<String, String> = HashMap::new();
            for r in rels_doc.root.elements() {
                rels.insert(
                    r.attr("Id").unwrap_or("").to_string(),
                    r.attr("Target").unwrap_or("").to_string(),
                );
            }
            let sheets_el = wb
                .root
                .child(NS, "sheets")
                .ok_or_else(|| Error::invalid_data("no sheets"))?;
            let sheets: Vec<&Element> =
                sheets_el.elements().filter(|e| e.is(NS, "sheet")).collect();
            let pick = sheets
                .iter()
                .find(|s| {
                    s.attr("name")
                        .is_some_and(|n| n.eq_ignore_ascii_case("Zones"))
                })
                .or_else(|| sheets.first())
                .ok_or_else(|| Error::invalid_data("no sheets"))?;
            let rid = pick
                .attr_ns(REL_NS, "id")
                .ok_or_else(|| Error::invalid_data("sheet without id"))?;
            let target = rels
                .get(rid)
                .ok_or_else(|| Error::invalid_data("relationship not found"))?;
            let path = match target.strip_prefix('/') {
                Some(_) => target.trim_start_matches('/').to_string(),
                None => format!("xl/{target}"),
            };
            let sheet = Self::load(&mut z, &path)?;
            let mut rows = Vec::new();
            let mut row_els: Vec<&Element> = Vec::new();
            sheet.root.descendants(NS, "row", &mut row_els);
            for r in row_els {
                let mut cells: Vec<String> = Vec::new();
                let mut next_col = 0usize;
                for c in r.elements().filter(|e| e.is(NS, "c")) {
                    let col = Self::column_index(c.attr("r"), next_col);
                    while cells.len() < col {
                        cells.push(String::new());
                    }
                    cells.push(Self::cell_text(c, &shared));
                    next_col = col + 1;
                }
                if cells.iter().any(|x| !x.is_empty()) {
                    rows.push(cells);
                }
            }
            Ok(rows)
        })()
        .map_err(wrap)
    }

    fn column_index(cell_ref: Option<&str>, fallback: usize) -> usize {
        let Some(r) = cell_ref.filter(|r| !r.is_empty()) else {
            return fallback;
        };
        let mut n = 0usize;
        let mut i = 0;
        for ch in r.chars() {
            if !ch.is_alphabetic() {
                break;
            }
            n = n * 26
                + (ch.to_ascii_uppercase() as usize)
                    .wrapping_sub('A' as usize)
                    .wrapping_add(1);
            i += 1;
        }
        if i == 0 {
            fallback
        } else {
            n.wrapping_sub(1)
        }
    }

    fn cell_text(c: &Element, shared: &[String]) -> String {
        let t = c.attr("t");
        if t == Some("inlineStr") {
            let mut ts = Vec::new();
            c.descendants(NS, "t", &mut ts);
            return ts
                .iter()
                .map(|x| x.value())
                .collect::<String>()
                .trim()
                .to_string();
        }
        let v = c.child(NS, "v").map(|e| e.value()).unwrap_or_default();
        if t == Some("s") {
            return match parse_int_loose(&v) {
                Some(i) if i >= 0 && (i as usize) < shared.len() => {
                    shared[i as usize].trim().to_string()
                }
                _ => String::new(),
            };
        }
        if t == Some("b") {
            return if v == "1" {
                "TRUE".into()
            } else {
                "FALSE".into()
            };
        }
        if t.is_none() || t == Some("n") {
            if let Ok(d) = v.trim().parse::<f64>() {
                if d == d.floor() && d.abs() < 1e9 {
                    return (d as i64).to_string(); // 1 not 1.0
                }
            }
        }
        v.trim().to_string()
    }

    // ---- turning a table into zone rows -----------------------------------------------------------------------------

    fn header_key(h: &str) -> Option<&'static str> {
        Some(match h {
            "zone" | "zonenumber" | "zoneno" | "number" | "no" | "num" | "input"
            | "inputnumber" | "zn" | "id" => "n",
            "name" | "zonename" | "description" | "label" | "text" | "location" => "name",
            "definition" | "zonedefinition" | "function" | "zonefunction" | "zonetype"
            | "alarmtype" | "alarmdefinition" => "def",
            "type" | "wiring" | "zonewiring" | "wiretype" | "circuit" | "circuittype" => "type",
            "area" | "partition" | "areanumber" => "area",
            _ => return None,
        })
    }

    fn interpret(table: &[Vec<String>]) -> Result<ZoneSheetResult> {
        let mut res = ZoneSheetResult::default();
        if table.is_empty() {
            res.warnings.push("The file has no rows.".into());
            return Ok(res);
        }
        // the header is the first row where at least one cell is a known column name and none is a bare zone number
        let mut cols: HashMap<&'static str, usize> = HashMap::new();
        let mut start = 0;
        let head = &table[0];
        let named: Vec<(&'static str, usize)> = head
            .iter()
            .enumerate()
            .filter_map(|(i, h)| Self::header_key(&norm(h)).map(|k| (k, i)))
            .collect();
        if !named.is_empty() && !head.iter().any(|h| parse_int_loose(h).is_some()) {
            for (k, i) in named {
                cols.entry(k).or_insert(i);
            }
            start = 1;
        } else {
            cols.extend([("n", 0), ("name", 1), ("def", 2), ("type", 3), ("area", 4)]);
        }
        if !cols.contains_key("n") && !cols.contains_key("name") {
            return Err(Error::invalid_data(
                "No Zone or Name column found. Put headers such as Zone, Name, Definition, Type, Area in the first row.",
            ));
        }
        let sequential = !cols.contains_key("n");
        if sequential {
            res.warnings.push("There is no Zone column, so the names are applied to zones 1, 2, 3 \u{2026} in row order.".into());
        }
        for (k, label) in [
            ("n", "Zone"),
            ("name", "Name"),
            ("def", "Definition"),
            ("type", "Type"),
            ("area", "Area"),
        ] {
            if cols.contains_key(k) {
                res.columns.push(label.into());
            }
        }
        let cell = |r: &Vec<String>, k: &str| -> String {
            match cols.get(k) {
                Some(&i) if i < r.len() => r[i].trim().to_string(),
                _ => String::new(),
            }
        };
        let mut seq = 0;
        for (line, r) in table.iter().enumerate().skip(start) {
            let wh = format!("row {}", line + 1);
            if r.iter().all(|c| c.trim().is_empty()) {
                continue;
            }
            if !r.is_empty() && r[0].trim_start().starts_with('#') {
                continue;
            }
            seq += 1;
            let n: i32;
            if sequential {
                n = seq;
            } else {
                let zs = cell(r, "n");
                if zs.is_empty() {
                    res.warnings.push(format!("{wh}: no zone number, skipped."));
                    continue;
                }
                match parse_zone_number(&zs) {
                    Some(v) => n = v,
                    None => {
                        res.warnings
                            .push(format!("{wh}: \"{zs}\" is not a zone number, skipped."));
                        continue;
                    }
                }
            }
            if !(1..=Self::MAX_ZONE).contains(&n) {
                res.warnings.push(format!(
                    "{wh}: zone {n} is outside 1-{}, skipped.",
                    Self::MAX_ZONE
                ));
                continue;
            }
            let (mut name, mut def, mut ty, mut area) = (None, None, None, None);
            if cols.contains_key("name") {
                let mut s = cell(r, "name");
                if !s.is_empty() {
                    if s.chars().count() > 16 {
                        let cut: String = s.chars().take(16).collect();
                        res.warnings.push(format!(
                            "{wh}: zone {n} name \"{s}\" is longer than 16 characters and was cut to \"{cut}\"."
                        ));
                        s = cut;
                    }
                    name = Some(s);
                }
            }
            if cols.contains_key("def") {
                let s = cell(r, "def");
                if !s.is_empty() {
                    def = Self::lookup(&s, &ModelMeta::ZONE_DEFINITIONS);
                    if def.is_none() {
                        res.warnings.push(format!(
                            "{wh}: zone {n} definition \"{s}\" is not recognised, left unchanged."
                        ));
                    }
                }
            }
            if cols.contains_key("type") {
                let s = cell(r, "type");
                if !s.is_empty() {
                    ty = Self::lookup(&s, &ModelMeta::ZONE_TYPES);
                    if ty.is_none() {
                        res.warnings.push(format!(
                            "{wh}: zone {n} type \"{s}\" is not recognised, left unchanged."
                        ));
                    }
                }
            }
            if cols.contains_key("area") {
                let s = cell(r, "area");
                if !s.is_empty() {
                    let digits: String = s
                        .chars()
                        .skip_while(|c| !c.is_ascii_digit())
                        .take_while(|c| c.is_ascii_digit())
                        .collect();
                    match digits.parse::<i32>() {
                        Ok(v) if (1..=8).contains(&v) => area = Some(v),
                        _ => res.warnings.push(format!(
                            "{wh}: zone {n} area \"{s}\" is not 1-8, left unchanged."
                        )),
                    }
                }
            }
            res.rows.push(ZoneSheetRow {
                n,
                name,
                definition: def,
                type_: ty,
                area,
            });
        }
        // a zone listed twice: the last row wins, and the user is told
        let mut order: Vec<i32> = Vec::new();
        let mut counts: HashMap<i32, usize> = HashMap::new();
        for r in &res.rows {
            let c = counts.entry(r.n).or_insert(0);
            if *c == 0 {
                order.push(r.n);
            }
            *c += 1;
        }
        for k in order {
            if counts[&k] > 1 {
                res.warnings.push(format!(
                    "Zone {k} appears {} times; the last row is used.",
                    counts[&k]
                ));
            }
        }
        Ok(res)
    }

    /// Finds a definition or type by its number ("3", "03") or its text, ignoring case, spaces and punctuation; accepts
    /// "03 = Burglar Perimeter Instant" and "Burglar Perimeter Instant".
    pub fn lookup(s: &str, names: &[&str]) -> Option<i32> {
        let s = s.trim();
        let mut number: Option<i32> = None;
        let mut key = norm(s);
        if let Some((v, rest)) = parse_lookup_prefix(s) {
            if (v as usize) < names.len() {
                number = Some(v);
            }
            key = norm(rest.unwrap_or(""));
            if key.is_empty() {
                return number;
            }
        }
        for (i, n) in names.iter().enumerate() {
            if norm(n) == key {
                return Some(i as i32);
            }
        }
        if key.chars().count() >= 4 {
            // an unambiguous start such as "Burglar Perimeter"
            let hits: Vec<usize> = (0..names.len())
                .filter(|&i| norm(names[i]).starts_with(&key))
                .collect();
            if hits.len() == 1 {
                return Some(hits[0] as i32);
            }
        }
        number
    }

    // ---- writing ----------------------------------------------------------------------------------------------------

    /// The rows to export: every zone that is not Disabled, or all of them.
    pub fn table(zones: &[ZoneDto], include_disabled: bool) -> Vec<Vec<String>> {
        let mut t = vec![["Zone", "Name", "Definition", "Type", "Area"]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()];
        let mut zs: Vec<&ZoneDto> = zones
            .iter()
            .filter(|z| include_disabled || z.definition != 0)
            .collect();
        zs.sort_by_key(|z| z.n);
        for z in zs {
            t.push(vec![
                z.n.to_string(),
                z.name.clone(),
                pick(&ModelMeta::ZONE_DEFINITIONS, z.definition),
                pick(&ModelMeta::ZONE_TYPES, z.type_),
                format!("Area {}", z.area),
            ]);
        }
        t
    }

    pub fn to_csv(table: &[Vec<String>]) -> Vec<u8> {
        let mut sb = String::from("\u{feff}"); // the byte order mark makes Excel read it as UTF-8
        for r in table {
            sb.push_str(&r.iter().map(|c| quote(c)).collect::<Vec<_>>().join(","));
            sb.push_str("\r\n");
        }
        sb.into_bytes()
    }

    pub fn to_xlsx(table: &[Vec<String>]) -> Result<Vec<u8>> {
        const DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";
        let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let mut add = |path: &str, xml: String| -> Result<()> {
            zw.start_file(path, opts)
                .map_err(|e| Error::invalid_data(e.to_string()))?;
            zw.write_all(xml.as_bytes())
                .map_err(|e| Error::invalid_data(e.to_string()))
        };
        add("[Content_Types].xml", format!("{DECL}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/><Override PartName=\"/xl/worksheets/sheet1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>\
<Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/></Types>"))?;
        add("_rels/.rels", format!("{DECL}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>"))?;
        add("xl/workbook.xml", format!("{DECL}<workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><sheets><sheet name=\"Zones\" sheetId=\"1\" r:id=\"rId1\"/></sheets></workbook>"))?;
        add("xl/_rels/workbook.xml.rels", format!("{DECL}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet1.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/></Relationships>"))?;
        add("xl/styles.xml", format!("{DECL}<styleSheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><fonts count=\"2\"><font><sz val=\"11\"/><name val=\"Calibri\"/></font><font><b/><sz val=\"11\"/><name val=\"Calibri\"/></font></fonts><fills count=\"2\"><fill><patternFill patternType=\"none\"/></fill><fill><patternFill patternType=\"gray125\"/></fill></fills><borders count=\"1\"><border><left/><right/><top/><bottom/><diagonal/></border></borders>\
<cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs><cellXfs count=\"2\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/><xf numFmtId=\"0\" fontId=\"1\" fillId=\"0\" borderId=\"0\" xfId=\"0\" applyFont=\"1\"/></cellXfs></styleSheet>"))?;
        let mut sb = format!("{DECL}<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetViews><sheetView workbookViewId=\"0\"><pane ySplit=\"1\" topLeftCell=\"A2\" activePane=\"bottomLeft\" state=\"frozen\"/></sheetView></sheetViews>\
<cols><col min=\"1\" max=\"1\" width=\"7\" customWidth=\"1\"/><col min=\"2\" max=\"2\" width=\"20\" customWidth=\"1\"/><col min=\"3\" max=\"3\" width=\"30\" customWidth=\"1\"/><col min=\"4\" max=\"4\" width=\"26\" customWidth=\"1\"/><col min=\"5\" max=\"5\" width=\"9\" customWidth=\"1\"/></cols><sheetData>");
        for (r, row) in table.iter().enumerate() {
            sb.push_str(&format!("<row r=\"{}\">", r + 1));
            for (c, v) in row.iter().enumerate() {
                let at = format!("{}{}", (b'A' + c as u8) as char, r + 1);
                match (r > 0 && c == 0).then(|| parse_int_loose(v)).flatten() {
                    Some(num) => sb.push_str(&format!("<c r=\"{at}\"><v>{num}</v></c>")),
                    None => sb.push_str(&format!(
                        "<c r=\"{at}\" t=\"inlineStr\"{}><is><t xml:space=\"preserve\">{}</t></is></c>",
                        if r == 0 { " s=\"1\"" } else { "" },
                        security_escape(v)
                    )),
                }
            }
            sb.push_str("</row>");
        }
        sb.push_str("</sheetData></worksheet>");
        add("xl/worksheets/sheet1.xml", sb)?;
        let cur = zw
            .finish()
            .map_err(|e| Error::invalid_data(e.to_string()))?;
        Ok(cur.into_inner())
    }
}

fn pick(a: &[&str], i: i32) -> String {
    if i >= 0 && (i as usize) < a.len() {
        a[i as usize].to_string()
    } else {
        i.to_string()
    }
}

fn quote(s: &str) -> String {
    if s.contains([',', '"', '\r', '\n']) || s != s.trim() {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// `SecurityElement.Escape`.
fn security_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '&' => o.push_str("&amp;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            c => o.push(c),
        }
    }
    o
}

/// `^(?:zone\s*)?0*(\d{1,3})$` (ignore case).
fn parse_zone_number(zs: &str) -> Option<i32> {
    let lower = zs.to_lowercase();
    let rest = match lower.strip_prefix("zone") {
        Some(r) => r.trim_start(),
        None => lower.as_str(),
    };
    if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let t = rest.trim_start_matches('0');
    if t.len() > 3 {
        return None;
    }
    Some(if t.is_empty() { 0 } else { t.parse().ok()? })
}

/// `^(\d{1,2})\s*(?:[=:\-–]\s*(.*))?$`: the number and the text after the separator, if any.
fn parse_lookup_prefix(s: &str) -> Option<(i32, Option<&str>)> {
    let digits = s.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 || digits > 2 {
        return None;
    }
    let v: i32 = s[..digits].parse().ok()?;
    let rest = s[digits..].trim_start();
    if rest.is_empty() {
        return Some((v, None));
    }
    let after = rest.strip_prefix(['=', ':', '-', '\u{2013}'])?;
    let text = after.trim_start();
    if text.contains('\n') {
        return None;
    }
    Some((v, Some(text)))
}

fn collect_t_skip_phonetic(e: &Element, out: &mut String) {
    for n in &e.children {
        if let Node::Elem(c) = n {
            if c.is(NS, "t") {
                if !e.is(NS, "rPh") {
                    out.push_str(&c.value());
                }
            } else {
                collect_t_skip_phonetic(c, out);
            }
        }
    }
}

impl ZoneSheetResult {
    /// The body `/api/zones/read` returns: `{"rows":[..],"columns":[..],"warnings":[..]}` (web-default JSON).
    pub fn to_json(&self) -> String {
        crate::json::to_string(self)
    }
}
