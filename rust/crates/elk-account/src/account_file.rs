//! An ElkRP "Export Account" file (VB `Write#` format, one value per line) with the sections this tool edits located:
//! Users, Areas, Keypads, Zones (+ 13 wireless zone-group flags) and Telephone numbers. All other lines are kept verbatim.
//!
//! Block layouts come from each form's Form_Export in the M1 plugin:
//! - Users: count, N x (code number, 28 bytes)
//! - Areas: count, N x (area number, 24 bytes)
//! - Keypads: count, N x (keypad number, 148 bytes), then 16 keypad-version values
//! - Zones: count, N x (zone number, 32 bytes), then 13 wireless zone-group flags (software 5.x and later)
//! - Wireless: 3 x 260 values (software 5.x and later), not edited
//! - Telephones: count, N x (telephone id, 88 bytes)
//!
//! Entries carry their own numbers and are stored in ascending order; a missing number is simply not stored.

use crate::automation::{self, AutomationBlock};
use crate::error::{is_num, parse_byte, Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub number: i32,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Section {
    pub name: &'static str,
    pub record_size: usize,
    pub max_number: i32,
    pub trailer_count: usize,
    pub entries: Vec<Entry>,
    pub trailer: Vec<String>,
}

impl Section {
    fn new(
        name: &'static str,
        record_size: usize,
        max_number: i32,
        trailer_count: usize,
    ) -> Section {
        Section {
            name,
            record_size,
            max_number,
            trailer_count,
            entries: Vec::new(),
            trailer: Vec::new(),
        }
    }

    pub fn get(&self, n: i32) -> Option<&Entry> {
        self.entries.iter().find(|e| e.number == n)
    }

    pub fn get_mut(&mut self, n: i32) -> Option<&mut Entry> {
        self.entries.iter_mut().find(|e| e.number == n)
    }

    /// The entry for `n`, added (zero filled, in number order) when missing.
    pub fn get_or_add(&mut self, n: i32) -> Result<&mut Entry> {
        if let Some(i) = self.entries.iter().position(|e| e.number == n) {
            return Ok(&mut self.entries[i]);
        }
        if n < 1 || n > self.max_number {
            return Err(Error::OutOfRange(format!(
                "{} number must be 1-{} (Parameter 'n')",
                self.name, self.max_number
            )));
        }
        let e = Entry {
            number: n,
            bytes: vec![0; self.record_size],
        };
        let i = match self.entries.iter().position(|x| x.number > n) {
            Some(i) => {
                self.entries.insert(i, e);
                i
            }
            None => {
                self.entries.push(e);
                self.entries.len() - 1
            }
        };
        Ok(&mut self.entries[i])
    }
}

#[derive(Debug, Clone)]
pub struct AccountFile {
    pub header: String,
    pub users: Section,
    pub areas: Section,
    pub keypads: Section,
    pub zones: Section,
    pub phones: Section,
    // Communicator ("Report Codes") block: area RCs (number + 20 bytes), user RCs (number + 4), zone RCs (number + 4),
    // 24 system RC bytes, 96 keypad RC bytes (6 per keypad).
    pub area_rcs: Section,
    pub user_rcs: Section,
    pub zone_rcs: Section,
    pub system_rc: [u8; 24],
    pub keypad_rc: [u8; 96],
    /// The automation items that follow the report codes, or None (older software, unexpected content: lines copied through).
    pub automation: Option<AutomationBlock>,

    eol: String,
    pub(crate) lines: Vec<String>,
    /// Replacement for the account details lines (header up to the users block), set by `set_details`.
    pub(crate) head: Option<Vec<String>>,
    rc_at: Option<usize>,
    rc_len: usize,
    wireless_at: Option<usize>,
    wireless_blocks: usize,
    pub(crate) users_at: usize,
    zones_end: usize,
    phones_at: Option<usize>,
    globals_at: Option<usize>,
    globals_len: usize,
    software_version: String,
    panel_type: String,
    account_id: String,
    code_length: Option<i32>,
}

/// Reads one block at line `i`. Returns the line after it, or None if the lines there do not have the block's shape.
fn try_block(l: &[String], i: usize, s: &Section, into: &mut Vec<Entry>) -> Option<usize> {
    into.clear();
    if i >= l.len() {
        return None;
    }
    let n = is_num(&l[i], 255)?;
    if n > s.max_number {
        return None;
    }
    let n = n as usize;
    let mut p = i + 1;
    let mut prev = 0;
    if p + (s.record_size + 1) * n > l.len() {
        return None;
    }
    for _ in 0..n {
        let num = is_num(&l[p], s.max_number)?;
        if num <= prev {
            return None;
        }
        prev = num;
        let mut b = vec![0u8; s.record_size];
        for (j, slot) in b.iter_mut().enumerate() {
            *slot = is_num(&l[p + 1 + j], 255)? as u8;
        }
        into.push(Entry {
            number: num,
            bytes: b,
        });
        p += s.record_size + 1;
    }
    Some(p)
}

fn take_trailer(l: &[String], at: usize, count: usize) -> Result<Vec<String>> {
    if at + count <= l.len() {
        Ok(l[at..at + count].to_vec())
    } else {
        Err(Error::invalid_data("file ends inside a section trailer"))
    }
}

/// `SplitCsvLine`: quotes toggle, commas inside quotes are kept, quote characters are dropped.
pub fn split_csv_line(line: &str) -> Vec<String> {
    let mut f = Vec::new();
    let mut sb = String::new();
    let mut q = false;
    for c in line.chars() {
        if c == '"' {
            q = !q;
        } else if c == ',' && !q {
            f.push(std::mem::take(&mut sb));
        } else {
            sb.push(c);
        }
    }
    f.push(sb);
    f
}

/// Decodes bytes as Latin-1 (every byte is the code point of the same value).
pub fn decode_latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

/// Encodes text as Latin-1; characters above U+00FF become '?'.
pub fn encode_latin1(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| {
            if (c as u32) < 256 {
                c as u32 as u8
            } else {
                b'?'
            }
        })
        .collect()
}

impl AccountFile {
    /// Software version text from the account header (field 18), e.g. "5.0.12".
    pub fn software_version(&self) -> &str {
        &self.software_version
    }
    pub fn panel_type(&self) -> &str {
        &self.panel_type
    }
    pub fn account_id(&self) -> &str {
        &self.account_id
    }
    /// User code length from Globals byte 0 bit 2 (set = 6 digits, else 4), or None if the globals could not be located.
    pub fn code_length(&self) -> Option<i32> {
        self.code_length
    }
    pub fn has_comm(&self) -> bool {
        self.rc_at.is_some()
    }
    /// The lines after the automation block, where ElkRP writes the M1XEP Setup block (`XEPConfig.Form_Export`, the last block of an export). `None` when the
    /// automation block was not located (older software), because then the position is not known.
    pub fn lines_after_automation(&self) -> Option<&[String]> {
        let at = self.rc_at?
            + self.rc_len
            + if self.automation.is_some() {
                automation::LINE_COUNT
            } else {
                return None;
            };
        self.lines.get(at..)
    }
    /// True when the telephone block was located (it may hold 0 entries).
    pub fn has_phone_section(&self) -> bool {
        self.phones_at.is_some()
    }

    fn empty() -> AccountFile {
        AccountFile {
            header: String::new(),
            users: Section::new("User", 28, 255, 0),
            areas: Section::new("Area", 24, 8, 0),
            keypads: Section::new("Keypad", 148, 16, 16),
            zones: Section::new("Zone", 32, 208, 13),
            phones: Section::new("Telephone", 88, 8, 0),
            area_rcs: Section::new("Area RC", 20, 8, 0),
            user_rcs: Section::new("User RC", 4, 255, 0),
            zone_rcs: Section::new("Zone RC", 4, 208, 0),
            system_rc: [0; 24],
            keypad_rc: [0; 96],
            automation: None,
            eol: "\r\n".into(),
            lines: Vec::new(),
            head: None,
            rc_at: None,
            rc_len: 0,
            wireless_at: None,
            wireless_blocks: 0,
            users_at: 0,
            zones_end: 0,
            phones_at: None,
            globals_at: None,
            globals_len: 0,
            software_version: String::new(),
            panel_type: String::new(),
            account_id: String::new(),
            code_length: None,
        }
    }

    /// Parses the file text (decode the bytes as Latin-1 first, see [`decode_latin1`]).
    pub fn parse(text: &str) -> Result<AccountFile> {
        let mut f = AccountFile::empty();
        f.eol = if text.contains("\r\n") {
            "\r\n".into()
        } else {
            "\n".into()
        };
        f.lines = text
            .split('\n')
            .map(|l| l.trim_end_matches('\r').to_string())
            .collect();
        if f.lines.last().is_some_and(|l| l.is_empty()) {
            f.lines.pop();
        }
        let l = std::mem::take(&mut f.lines);
        f.header = l.first().cloned().unwrap_or_default();
        let hf = split_csv_line(&f.header);
        if hf.len() > 18 {
            f.account_id = hf[0].clone();
            f.panel_type = hf[16].clone();
            f.software_version = hf[17].clone();
        }
        let major = f.software_version.split('.').next().unwrap_or("");
        let v5 = major.trim().parse::<f64>().is_ok_and(|m| m >= 5.0);
        let zone_trailer = if v5 { 13 } else { 0 };

        // Users -> Areas -> Keypads -> Zones are consecutive, which makes the chain easy to find reliably. Take the match with the most entries.
        let (mut best_at, mut best_score, mut tie) = (None::<usize>, -1i64, false);
        let mut tmp = Vec::new();
        let (u, a, k, z) = (
            Section::new("", 28, 255, 0),
            Section::new("", 24, 8, 0),
            Section::new("", 148, 16, 0),
            Section::new("", 32, 208, 0),
        );
        for i in 1..l.len() {
            let Some(e1) = try_block(&l, i, &u, &mut tmp) else {
                continue;
            };
            let nu = tmp.len();
            let Some(e2) = try_block(&l, e1, &a, &mut tmp) else {
                continue;
            };
            let na = tmp.len();
            let Some(e3) = try_block(&l, e2, &k, &mut tmp) else {
                continue;
            };
            let nk = tmp.len();
            let Some(e4) = try_block(&l, e3 + 16, &z, &mut tmp) else {
                continue;
            };
            let _ = e4;
            let nz = tmp.len();
            let score = (nu + na + nk + nz) as i64;
            if score > best_score {
                best_score = score;
                best_at = Some(i);
                tie = false;
            } else if score == best_score {
                tie = true;
            }
        }
        let best_at = match best_at {
            Some(b) if !tie => b,
            _ => {
                return Err(Error::invalid_data(
                    "Could not find the Users/Areas/Keypads/Zones sections in this file. Is it an M1 / M1G account export?",
                ))
            }
        };

        f.users_at = best_at;
        let mut at = best_at;
        for which in 0..4 {
            let s = match which {
                0 => &mut f.users,
                1 => &mut f.areas,
                2 => &mut f.keypads,
                _ => &mut f.zones,
            };
            let mut entries = Vec::new();
            at = try_block(&l, at, s, &mut entries)
                .ok_or_else(|| Error::invalid_data("section vanished"))?;
            s.entries = entries;
            if s.trailer_count > 0 {
                let tc = if which == 3 {
                    zone_trailer
                } else {
                    s.trailer_count
                };
                s.trailer = take_trailer(&l, at, tc)?;
                at += tc;
            }
        }
        f.zones_end = at;

        // Telephones: after the 780 wireless values, globals and dawn/dusk (sizes not needed). Find the first block whose ids start at 1.
        let wireless_end = at + if v5 { 780 } else { 260 };
        if wireless_end <= l.len() {
            f.wireless_at = Some(at);
            f.wireless_blocks = if v5 { 3 } else { 1 };
        }
        if wireless_end < l.len() {
            if let Some(g0) = is_num(&l[wireless_end], 255) {
                f.code_length = Some(if g0 & 4 != 0 { 6 } else { 4 });
                f.globals_at = Some(wireless_end);
            }
        }
        // Exact position from the ElkRP exporter: Globals writes 48 values when the software string starts above "4.3" (else 36),
        // then Dawn/Dusk writes 102. Try that first; the shape search below is only a fallback.
        let first3: String = f.software_version.chars().take(3).collect();
        let globals_len = if first3.as_str() > "4.3" { 48 } else { 36 };
        f.globals_len = globals_len;
        let exact_phones = wireless_end + globals_len + 102;
        let mut search_from = wireless_end;
        let ph_proto = Section::new("", 88, 8, 0);
        if exact_phones < l.len() {
            let mut ph0 = Vec::new();
            let e0 = try_block(&l, exact_phones, &ph_proto, &mut ph0);
            // An account with no telephones writes just "0" here; accept it so the search below cannot latch onto the report-code block that follows.
            if e0.is_some() && (ph0.is_empty() || ph0[0].number == 1) {
                f.phones_at = Some(exact_phones);
                f.phones.entries = ph0;
                search_from = l.len();
            }
        }
        for i in search_from..l.len() {
            let mut ph = Vec::new();
            if try_block(&l, i, &ph_proto, &mut ph).is_none() || ph.is_empty() || ph[0].number != 1
            {
                continue;
            }
            f.phones_at = Some(i);
            f.phones.entries = ph;
            break;
        }
        // Communicator block: after the telephones. Take the candidate with the most entries (the first one on a tie).
        let original_phone_count = match f.phones_at {
            Some(p) => l[p].trim().parse::<usize>().unwrap_or(0),
            None => 0,
        };
        let rc_from = match f.phones_at {
            Some(p) => p + 1 + 89 * original_phone_count,
            None => wireless_end,
        };
        let (ap, up, zp) = (
            Section::new("", 20, 8, 0),
            Section::new("", 4, 255, 0),
            Section::new("", 4, 208, 0),
        );
        let mut rc_best: i64 = -1;
        for i in rc_from..l.len() {
            let (mut ae, mut ue, mut ze) = (Vec::new(), Vec::new(), Vec::new());
            let Some(e1) = try_block(&l, i, &ap, &mut ae) else {
                continue;
            };
            let Some(e2) = try_block(&l, e1, &up, &mut ue) else {
                continue;
            };
            let Some(e3) = try_block(&l, e2, &zp, &mut ze) else {
                continue;
            };
            if e3 + 120 > l.len() {
                continue;
            }
            if !(0..120).all(|j| is_num(&l[e3 + j], 255).is_some()) {
                continue;
            }
            let score = (ae.len() + ue.len() + ze.len()) as i64;
            if score <= rc_best {
                continue;
            }
            rc_best = score;
            f.rc_at = Some(i);
            f.rc_len = e3 + 120 - i;
            f.area_rcs.entries = ae;
            f.user_rcs.entries = ue;
            f.zone_rcs.entries = ze;
            for j in 0..24 {
                f.system_rc[j] = parse_byte(&l[e3 + j])?;
            }
            for j in 0..96 {
                f.keypad_rc[j] = parse_byte(&l[e3 + 24 + j])?;
            }
        }
        if let Some(rc) = f.rc_at {
            f.automation = AutomationBlock::try_parse(&l, rc + f.rc_len, &f.software_version);
        }
        f.lines = l;
        Ok(f)
    }

    fn original_phone_count(&self) -> usize {
        match self.phones_at {
            Some(p) => self.lines[p].trim().parse::<usize>().unwrap_or(0),
            None => 0,
        }
    }

    /// Writes the file back, replacing the edited sections' lines and copying every other line unchanged.
    pub fn to_text(&self) -> String {
        let mut out: Vec<String> = Vec::with_capacity(self.lines.len() + 64);
        // untouched head (header + account details lines) up to the users block
        match &self.head {
            Some(h) => out.extend(h.iter().cloned()),
            None => out.extend(self.lines.iter().take(self.users_at).cloned()),
        }
        fn emit(out: &mut Vec<String>, s: &Section, trailer_count: usize) {
            out.push(s.entries.len().to_string());
            for e in &s.entries {
                out.push(e.number.to_string());
                out.extend(e.bytes.iter().map(|b| b.to_string()));
            }
            out.extend(s.trailer.iter().take(trailer_count).cloned());
        }
        emit(&mut out, &self.users, 0);
        emit(&mut out, &self.areas, 0);
        emit(&mut out, &self.keypads, 16);
        emit(&mut out, &self.zones, self.zones.trailer.len());
        let mut pos = self.zones_end;
        let copy = |out: &mut Vec<String>, pos: &mut usize, up_to: usize| {
            if up_to > *pos {
                out.extend(self.lines.iter().skip(*pos).take(up_to - *pos).cloned());
                *pos = up_to;
            }
        };
        if let Some(p) = self.phones_at {
            copy(&mut out, &mut pos, p);
            emit(&mut out, &self.phones, 0);
            pos = p + 1 + 89 * self.original_phone_count();
        }
        if let Some(rc) = self.rc_at {
            copy(&mut out, &mut pos, rc);
            emit(&mut out, &self.area_rcs, 0);
            emit(&mut out, &self.user_rcs, 0);
            emit(&mut out, &self.zone_rcs, 0);
            out.extend(self.system_rc.iter().map(|b| b.to_string()));
            out.extend(self.keypad_rc.iter().map(|b| b.to_string()));
            pos = rc + self.rc_len;
            if let Some(a) = &self.automation {
                a.emit(&mut out);
                pos += automation::LINE_COUNT;
            }
        }
        copy(&mut out, &mut pos, self.lines.len());
        let mut s = out.join(&self.eol);
        s.push_str(&self.eol);
        s
    }

    /// The wireless receiver blocks (3 x 260 values for software 5.x and later, else 1), or None if they were not located.
    pub fn wireless(&self) -> Result<Option<Vec<Vec<u8>>>> {
        let Some(at) = self.wireless_at else {
            return Ok(None);
        };
        let mut blocks = Vec::new();
        for b in 0..self.wireless_blocks {
            blocks.push(
                self.lines
                    .iter()
                    .skip(at + b * 260)
                    .take(260)
                    .map(|l| parse_byte(l))
                    .collect::<Result<Vec<u8>>>()?,
            );
        }
        Ok(Some(blocks))
    }

    /// Overwrites one wireless block (0-based) in place.
    pub fn set_wireless(&mut self, block: usize, w: &[u8]) -> Result<()> {
        let at = match self.wireless_at {
            Some(a) if block < self.wireless_blocks => a,
            _ => return Err(Error::InvalidOperation("no such wireless block".into())),
        };
        if w.len() != 260 {
            return Err(Error::Argument("a wireless block has 260 values".into()));
        }
        for (i, v) in w.iter().enumerate() {
            self.lines[at + block * 260 + i] = v.to_string();
        }
        Ok(())
    }

    /// The globals values (48 bytes for software above 4.3, else 36), or None if they were not located.
    pub fn globals(&self) -> Result<Option<Vec<u8>>> {
        let Some(at) = self.globals_at else {
            return Ok(None);
        };
        self.lines
            .iter()
            .skip(at)
            .take(self.globals_len)
            .map(|l| parse_byte(l))
            .collect::<Result<Vec<u8>>>()
            .map(Some)
    }

    /// Overwrites the globals lines in place (same length) and refreshes [`AccountFile::code_length`].
    pub fn set_globals(&mut self, g: &[u8]) -> Result<()> {
        let Some(at) = self.globals_at else {
            return Err(Error::InvalidOperation(
                "globals were not located in this file".into(),
            ));
        };
        if g.len() < self.globals_len {
            return Err(Error::Argument(format!(
                "need {} globals bytes, got {}",
                self.globals_len,
                g.len()
            )));
        }
        for (i, v) in g.iter().take(self.globals_len).enumerate() {
            self.lines[at + i] = v.to_string();
        }
        self.code_length = Some(if g[0] & 4 != 0 { 6 } else { 4 });
        Ok(())
    }
}
