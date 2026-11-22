//! Fills the "Alarm Zone List ELK" worksheet (the commissioning checklist, embedded as `AlarmZoneTemplate.xlsx`) from an account.
//! The template is patched in place, so the logo, styles, check boxes and conditional formatting are kept exactly as supplied.
//! Filled: title (account name), created/modified date, and for every zone the name, definition code (Type) and area.
//! Left blank on purpose because the account does not hold them: wire number, power source, location, expander power, notes, sign-offs.
//! Rows the template uses for the second half of a double contact are hidden (unhide them to use them).

use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read, Write};

use crate::account_model::{AccountModel, ZoneDto};
use crate::error::{parse_int_loose, Error, Result};
use crate::xml::{Document, Element, Node, XML_NS};

const NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const TEMPLATE_LAST_ZONE_ROW: i32 = 172;
const TEMPLATE_LAST_ROW: i32 = 175;
const TEMPLATE_LAST_ZONE: i32 = 128;
const MAX_ZONE: i32 = 208;

static TEMPLATE: &[u8] = include_bytes!("AlarmZoneTemplate.xlsx");

pub struct AlarmZoneList;

impl AlarmZoneList {
    pub fn template_bytes() -> &'static [u8] {
        TEMPLATE
    }

    /// Builds the .xlsx. `title` defaults to the account name; `now` (the date written to the sheet) defaults to today (local time).
    pub fn build(
        model: &AccountModel,
        title: Option<&str>,
        now: Option<chrono::NaiveDate>,
    ) -> Result<Vec<u8>> {
        let date = now
            .unwrap_or_else(|| chrono::Local::now().date_naive())
            .format("%Y-%m-%d")
            .to_string();
        let mut zones: HashMap<i32, &ZoneDto> = HashMap::new();
        for z in &model.zones {
            zones.entry(z.n).or_insert(z);
        }
        let used = |z: &ZoneDto| z.definition != 0 || !z.name.chars().all(char::is_whitespace);
        let max_used = zones
            .values()
            .filter(|z| used(z))
            .map(|z| z.n)
            .max()
            .unwrap_or(0)
            .max(0);

        let mut src = zip::ZipArchive::new(Cursor::new(TEMPLATE))
            .map_err(|e| Error::invalid_data(e.to_string()))?;
        let sheet_path = "xl/worksheets/sheet1.xml";
        let book_path = "xl/workbook.xml";
        let mut sheet = load_xml(&mut src, sheet_path)?;
        let mut book = load_xml(&mut src, book_path)?;
        let sst = load_xml(&mut src, "xl/sharedStrings.xml")?;
        let strings: Vec<String> = sst
            .root
            .elements()
            .filter(|e| e.is(NS, "si"))
            .map(|si| {
                let mut ts = Vec::new();
                si.descendants(NS, "t", &mut ts);
                ts.iter().map(|t| t.value()).collect::<String>()
            })
            .collect();

        let root = &mut sheet.root;
        let data = root
            .child_mut(NS, "sheetData")
            .ok_or_else(|| Error::invalid_data("sheetData missing"))?;

        // header block
        {
            let r1 = row_mut(data, 1)?;
            set_text(get_cell(r1, "B"), title.unwrap_or(&model.account));
            set_text(get_cell(row_mut(data, 2)?, "B"), &date);
            set_text(get_cell(row_mut(data, 3)?, "B"), &date);
        }

        // the wireless block's header says "Type" twice; the second column is the area (shared string 7)
        {
            let r27 = row_mut(data, 27)?;
            let idx = strings
                .iter()
                .position(|s| s == "Area")
                .map(|i| i as i64)
                .unwrap_or(-1);
            if let Some(f27) = find_cell_mut(r27, "F") {
                if text_of(Some(f27), &strings) == "Type" {
                    f27.set_attr("t", "s");
                    if let Some(v) = f27.child_mut(NS, "v") {
                        v.set_value(&idx.to_string());
                    } else {
                        return Err(Error::invalid_data("cell F27 has no value"));
                    }
                }
            }
        }

        let mut hidden: HashSet<i32> = HashSet::new();
        for row in data.elements_mut().filter(|e| e.is(NS, "row")) {
            let n = row_no(row);
            let a_text = text_of(find_cell(row, "A"), &strings);
            if a_text.starts_with("Location:") {
                set_text(find_cell_mut(row, "A").expect("A"), "Location:");
            } else if a_text.starts_with("Expander Powered From:") {
                set_text(
                    find_cell_mut(row, "A").expect("A"),
                    "Expander Powered From:",
                );
            } else if let (Some(zn), true) = (
                parse_int_loose(&a_text),
                find_cell(row, "A").is_some_and(|a| a.attr("t").is_none()) && n >= 7,
            ) {
                let z = zones.get(&zn).copied();
                let is_used = z.is_some_and(used);
                set_text(
                    get_cell(row, "B"),
                    if is_used { z.unwrap().name.trim() } else { "" },
                );
                match z {
                    Some(z) if is_used && z.definition != 0 => {
                        set_text(get_cell(row, "E"), &two_digits(z.definition));
                        set_number(get_cell(row, "F"), z.area);
                    }
                    _ => {
                        set_text(get_cell(row, "E"), "");
                        set_text(get_cell(row, "F"), "");
                    }
                }
                clear_sample(row);
            } else if n >= 7
                && a_text.is_empty()
                && find_cell(row, "G").is_some_and(|g| g.attr("t") == Some("b"))
                && !text_of(find_cell(row, "B"), &strings)
                    .chars()
                    .all(char::is_whitespace)
            {
                // second half of a double contact in the sample project: blank it and hide it
                set_text(get_cell(row, "B"), "");
                clear_sample(row);
                row.set_attr("hidden", "1");
                hidden.insert(n);
            }
        }

        // growth needs prototype rows from the (already filled) sheet
        let last_row: i32 = if max_used <= TEMPLATE_LAST_ZONE {
            // the template ends with the labels of an empty expander 6: drop them
            data.children.retain(|n| !matches!(n, Node::Elem(e) if e.is(NS, "row") && row_no(e) > TEMPLATE_LAST_ZONE_ROW));
            let merges = root
                .child_mut(NS, "mergeCells")
                .ok_or_else(|| Error::invalid_data("mergeCells missing"))?;
            merges.children.retain(
                |n| !matches!(n, Node::Elem(m) if merge_row(m.attr("ref").unwrap_or("")) > TEMPLATE_LAST_ZONE_ROW),
            );
            TEMPLATE_LAST_ZONE_ROW
        } else {
            let (proto, header) = {
                let d = root.child(NS, "sheetData").unwrap();
                let find = |no: i32| {
                    d.elements()
                        .find(|r| r.is(NS, "row") && row_no(r) == no)
                        .cloned()
                        .ok_or_else(|| Error::invalid_data("template row missing"))
                };
                (find(168)?, find(156)?)
            };
            let mut new_rows: Vec<Element> = Vec::new();
            let mut new_merges: Vec<String> = Vec::new();
            let mut new_breaks: Vec<i32> = vec![TEMPLATE_LAST_ZONE_ROW];
            let mut next = TEMPLATE_LAST_ROW + 1;
            let mut first = 129;
            let mut exp = 6;
            while first <= MAX_ZONE && first <= max_used.max(129) {
                if exp > 6 {
                    // labels for expanders 7 and up
                    new_breaks.push(next - 1);
                    let rows_add = [
                        ("Location:", 72, true),
                        ("Expander Powered From:", 40, true),
                        ("", 44, false),
                    ];
                    let exp_label = format!("Expander: {exp}");
                    for (i, (txt, style, merge)) in rows_add.iter().enumerate() {
                        let mut lr = Element::new(NS, "row");
                        lr.set_attr("r", &next.to_string());
                        lr.set_attr("spans", "1:13");
                        let label: &str = if i == 2 { &exp_label } else { txt };
                        lr.add(styled_cell("A", next, *style, Some(label)));
                        lr.add(styled_cell("B", next, *style, None));
                        lr.add(styled_cell("C", next, 41, None));
                        new_rows.push(lr);
                        if *merge {
                            new_merges.push(format!("A{next}:B{next}"));
                        }
                        next += 1;
                    }
                }
                new_rows.push(clone_row(&header, next));
                next += 1;
                let mut k = 0;
                while k < 16 && first + k <= MAX_ZONE {
                    let zn = first + k;
                    let mut zr = clone_row(&proto, next);
                    set_number(get_cell(&mut zr, "A"), zn);
                    let z = zones.get(&zn).copied();
                    let is_used = z.is_some_and(used);
                    set_text(
                        get_cell(&mut zr, "B"),
                        if is_used { z.unwrap().name.trim() } else { "" },
                    );
                    if let Some(z) = z {
                        if is_used && z.definition != 0 {
                            set_text(get_cell(&mut zr, "E"), &two_digits(z.definition));
                            set_number(get_cell(&mut zr, "F"), z.area);
                        }
                    }
                    new_rows.push(zr);
                    next += 1;
                    k += 1;
                }
                exp += 1;
                first += 16;
            }
            let data = root.child_mut(NS, "sheetData").unwrap();
            for r in new_rows {
                data.add(r);
            }
            if let Some(b) = root.child_mut(NS, "rowBreaks") {
                for id in new_breaks {
                    let mut brk = Element::new(NS, "brk");
                    brk.set_attr("id", &id.to_string());
                    brk.set_attr("max", "12");
                    brk.set_attr("man", "1");
                    b.add(brk);
                }
            }
            let merges = root
                .child_mut(NS, "mergeCells")
                .ok_or_else(|| Error::invalid_data("mergeCells missing"))?;
            for m in new_merges {
                let mut e = Element::new(NS, "mergeCell");
                e.set_attr("ref", &m);
                merges.add(e);
            }
            next - 1
        };
        let merges = root
            .child_mut(NS, "mergeCells")
            .ok_or_else(|| Error::invalid_data("mergeCells missing"))?;
        let mc = merges.elements().count();
        merges.set_attr("count", &mc.to_string());
        if let Some(b) = root.child_mut(NS, "rowBreaks") {
            let c = b.elements().count().to_string();
            b.set_attr("count", &c);
            b.set_attr("manualBreakCount", &c);
        }
        root.child_mut(NS, "dimension")
            .ok_or_else(|| Error::invalid_data("dimension missing"))?
            .set_attr("ref", &format!("A1:N{last_row}"));
        // the template remembered the cursor on H29; start at the top
        if let Some(sel) = root.first_descendant_mut(NS, "selection") {
            sel.set_attr("activeCell", "A1");
            sel.set_attr("sqref", "A1");
        }
        book.root
            .for_each_descendant_mut(NS, "definedName", &mut |dn| {
                if dn.attr("name") == Some("_xlnm.Print_Area") {
                    dn.set_value(&format!("'Alarm Zone List ELK'!$A$1:$M${last_row}"));
                }
            });

        let sheet_xml = sheet.to_string_with_decl();
        let book_xml = book.to_string_with_decl();
        let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for i in 0..src.len() {
            let mut e = src
                .by_index(i)
                .map_err(|e| Error::invalid_data(e.to_string()))?;
            let name = e.name().to_string();
            if e.is_dir() {
                out.add_directory(name, opts)
                    .map_err(|e| Error::invalid_data(e.to_string()))?;
                continue;
            }
            out.start_file(name.clone(), opts)
                .map_err(|e| Error::invalid_data(e.to_string()))?;
            if name == sheet_path {
                out.write_all(sheet_xml.as_bytes())
            } else if name == book_path {
                out.write_all(book_xml.as_bytes())
            } else {
                let mut buf = Vec::new();
                e.read_to_end(&mut buf)
                    .map_err(|e| Error::invalid_data(e.to_string()))?;
                out.write_all(&buf)
            }
            .map_err(|e| Error::invalid_data(e.to_string()))?;
        }
        Ok(out
            .finish()
            .map_err(|e| Error::invalid_data(e.to_string()))?
            .into_inner())
    }
}

/// `int.ToString("00")`.
fn two_digits(v: i32) -> String {
    if v < 0 {
        format!("-{:02}", -(v as i64))
    } else {
        format!("{v:02}")
    }
}

// ---- xml helpers -----------------------------------------------------------------------------------------------------

fn load_xml(z: &mut zip::ZipArchive<Cursor<&[u8]>>, path: &str) -> Result<Document> {
    let mut e = z
        .by_name(path)
        .map_err(|e| Error::invalid_data(e.to_string()))?;
    let mut buf = Vec::new();
    e.read_to_end(&mut buf)
        .map_err(|e| Error::invalid_data(e.to_string()))?;
    Document::parse(&String::from_utf8_lossy(&buf))
}

fn row_no(r: &Element) -> i32 {
    r.attr("r").and_then(|s| s.parse().ok()).unwrap_or(0)
}

fn row_mut(data: &mut Element, no: i32) -> Result<&mut Element> {
    data.elements_mut()
        .find(|r| r.is(NS, "row") && row_no(r) == no)
        .ok_or_else(|| Error::invalid_data(format!("template row {no} missing")))
}

fn merge_row(rf: &str) -> i32 {
    rf.split(':')
        .next()
        .unwrap_or("")
        .chars()
        .filter(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

fn col_index(col: &str) -> i32 {
    col.chars()
        .fold(0, |a, ch| a * 26 + (ch as i32 - 'A' as i32 + 1))
}

fn col_of(c: &Element) -> String {
    c.attr("r")
        .unwrap_or("")
        .chars()
        .take_while(|ch| ch.is_alphabetic())
        .collect()
}

fn find_cell<'a>(row: &'a Element, col: &str) -> Option<&'a Element> {
    let want = format!("{col}{}", row.attr("r").unwrap_or(""));
    row.elements()
        .find(|c| c.is(NS, "c") && c.attr("r") == Some(want.as_str()))
}

fn find_cell_mut<'a>(row: &'a mut Element, col: &str) -> Option<&'a mut Element> {
    let want = format!("{col}{}", row.attr("r").unwrap_or(""));
    row.elements_mut()
        .find(|c| c.is(NS, "c") && c.attr("r") == Some(want.as_str()))
}

/// The cell in a column, created in column order when the row has none.
fn get_cell<'a>(row: &'a mut Element, col: &str) -> &'a mut Element {
    let rn = row.attr("r").unwrap_or("").to_string();
    let want = col_index(col);
    let mut at: Option<usize> = None;
    let mut insert = false;
    for (i, n) in row.children.iter().enumerate() {
        if let Node::Elem(c) = n {
            if !c.is(NS, "c") {
                continue;
            }
            let ci = col_index(&col_of(c));
            if ci == want {
                at = Some(i);
                break;
            }
            if ci > want {
                at = Some(i);
                insert = true;
                break;
            }
        }
    }
    let idx = match at {
        Some(i) if !insert => i,
        other => {
            let mut nc = Element::new(NS, "c");
            nc.set_attr("r", &format!("{col}{rn}"));
            match other {
                Some(i) => {
                    row.children.insert(i, Node::Elem(nc));
                    i
                }
                None => {
                    row.children.push(Node::Elem(nc));
                    row.children.len() - 1
                }
            }
        }
    };
    match &mut row.children[idx] {
        Node::Elem(e) => e,
        Node::Text(_) => unreachable!(),
    }
}

/// Removes element children other than formulas (text nodes stay), as `c.Elements().Where(name != f).Remove()` does.
fn clear_non_f(c: &mut Element) {
    let had = !c.children.is_empty();
    c.children
        .retain(|n| !matches!(n, Node::Elem(e) if !e.is(NS, "f")));
    if had && c.children.is_empty() {
        c.explicit_end = false;
    }
}

fn set_text(c: &mut Element, text: &str) {
    c.remove_attr("t");
    clear_non_f(c);
    if text.is_empty() {
        return;
    }
    c.set_attr("t", "inlineStr");
    let mut is = Element::new(NS, "is");
    let mut t = Element::new(NS, "t");
    t.set_attr_ns("xml", XML_NS, "space", "preserve");
    t.children.push(Node::Text(text.to_string()));
    t.explicit_end = true;
    is.add(t);
    is.explicit_end = true;
    c.add(is);
    c.explicit_end = true;
}

fn set_number(c: &mut Element, v: i32) {
    c.remove_attr("t");
    clear_non_f(c);
    let mut e = Element::new(NS, "v");
    e.children.push(Node::Text(v.to_string()));
    e.explicit_end = true;
    c.add(e);
    c.explicit_end = true;
}

fn styled_cell(col: &str, row: i32, style: i32, text: Option<&str>) -> Element {
    let mut c = Element::new(NS, "c");
    c.set_attr("r", &format!("{col}{row}"));
    c.set_attr("s", &style.to_string());
    if let Some(t) = text {
        set_text(&mut c, t);
    }
    c
}

fn text_of(c: Option<&Element>, strings: &[String]) -> String {
    let Some(c) = c else { return String::new() };
    match c.attr("t") {
        Some("s") => {
            let v = c.child(NS, "v").map(|v| v.value()).unwrap_or_default();
            v.trim()
                .parse::<usize>()
                .ok()
                .and_then(|i| strings.get(i))
                .cloned()
                .unwrap_or_default()
        }
        Some("inlineStr") => c.value(),
        _ => c.child(NS, "v").map(|v| v.value()).unwrap_or_default(),
    }
}

/// Clears the hand-filled columns of a zone row (wire number, power source, checked-by, notes) and unticks both check boxes.
fn clear_sample(row: &mut Element) {
    for col in ["C", "D", "I", "L", "M"] {
        if let Some(c) = row
            .elements_mut()
            .find(|x| x.is(NS, "c") && col_of(x) == col)
        {
            set_text(c, "");
        }
    }
    for col in ["G", "J"] {
        if let Some(c) = row
            .elements_mut()
            .find(|x| x.is(NS, "c") && col_of(x) == col)
        {
            c.set_attr("t", "b");
            c.children.clear();
            let mut v = Element::new(NS, "v");
            v.children.push(Node::Text("0".into()));
            v.explicit_end = true;
            c.add(v);
            c.explicit_end = true;
        }
    }
}

/// Copy of a row moved to a new row number (formulas dropped: shared formula ids do not carry over).
fn clone_row(row: &Element, n: i32) -> Element {
    let mut copy = row.clone();
    copy.set_attr("r", &n.to_string());
    copy.remove_descendants(NS, "f");
    for c in copy.elements_mut().filter(|c| c.is(NS, "c")) {
        let col = col_of(c);
        c.set_attr("r", &format!("{col}{n}"));
        if c.attr("t") == Some("str") {
            c.remove_attr("t");
        }
        let had = !c.children.is_empty();
        c.children
            .retain(|x| !matches!(x, Node::Elem(v) if v.is(NS, "v") && v.value().is_empty()));
        if had && c.children.is_empty() {
            c.explicit_end = false;
        }
    }
    copy.remove_attr("hidden");
    copy
}
