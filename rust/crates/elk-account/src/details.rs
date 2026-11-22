//! The "Account Details" page of ElkRP: the 25-field account header record and the 47 lines that follow it (serial number,
//! RP access codes and MAC address obfuscated with ElkRP's XOR key, passphrase, network adapter, non-secure flags).
//! Layout from ElkRP's `Account_Details.Form_Export`:
//!
//! header fields: 1 account, 2 name, 3 address 1, 4 address 2, 5 city, 6 state, 7 zip, 8 voice, 9 fax, 10 system telephone,
//! 11 system URL/IP, 12 port, 13 C1M1 port(s), 14 e-mail, 15 web page, 16 folder (group), 17 system type, 18 firmware,
//! 19 event list version, 20 hardware, 21 boot, 22 voice list version, 23 date installed, 24 notes, 25 attributes.
//! Then: serial (8 lines), RP code, previous RP code 1, 2, 3 (6 lines each), MAC (12), passphrase, network adapter
//! (0 M1XEP, 1 C1M1, 2 M1XIP) and the non-secure flags (bit 1 M1XEP, bit 2 M1XIP).
//!
//! Only the fields ElkRP lets the user edit are written back, and only the ones that changed: every other byte of the head
//! (quoting, the obfuscated lines, the passphrase) is kept as it was.

use serde::{Deserialize, Serialize};

use crate::account_file::AccountFile;
use crate::error::{Error, Result};

const KEY: &[u8] = b"ElkRP Copyright 2004 Elk Products";
const FIELDS: usize = 25;
/// Lines after the header record: 8 serial + 4 x 6 RP codes + 12 MAC + passphrase + adapter + non-secure flags.
const AFTER: usize = 47;

/// The account details as the page shows them. Fields marked read-only are shown but never written back.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DetailsDto {
    pub name: String,
    pub address1: String,
    pub address2: String,
    pub city: String,
    pub state: String,
    pub zip: String,
    pub voice: String,
    pub fax: String,
    pub email: String,
    pub web: String,
    pub folder: String,
    pub notes: String,
    pub installed: String,
    pub system_phone: String,
    pub system_url: String,
    pub port: String,
    pub port_c: String,
    /// 0 = M1XEP, 1 = C1M1, 2 = M1XIP.
    pub network_adapter: i32,
    /// Bit 1 = connect to the M1XEP non-secure, bit 2 = to the M1XIP non-secure.
    pub non_secure: i32,
    // read-only
    pub event_list: String,
    pub hardware: String,
    pub boot: String,
    pub voice_list: String,
    pub serial: String,
    pub rp_code: String,
}

/// Field limits (characters) from the text boxes of ElkRP's form.
const LIMITS: [(&str, usize); 14] = [
    ("Name", 50),
    ("Address 1", 50),
    ("Address 2", 50),
    ("City", 30),
    ("State/Province", 3),
    ("Zip/Post Code", 10),
    ("Folder", 100),
    ("Voice telephone", 20),
    ("Fax", 20),
    ("E-mail", 30),
    ("Web page", 30),
    ("Notes", 255),
    ("System telephone", 20),
    ("System URL/IP", 50),
];

/// Splits a line at the commas outside quotes, keeping the quote characters (the raw text of each field).
fn raw_fields(s: &str) -> Vec<String> {
    let mut f = Vec::new();
    let mut sb = String::new();
    let mut q = false;
    for c in s.chars() {
        if c == '"' {
            q = !q;
            sb.push(c);
        } else if c == ',' && !q {
            f.push(std::mem::take(&mut sb));
        } else {
            sb.push(c);
        }
    }
    f.push(sb);
    f
}

fn plain(raw: &str) -> String {
    raw.chars().filter(|&c| c != '"').collect()
}

/// Number of lines the header record spans (a note with line breaks continues on the following lines until the quotes balance).
fn header_span(lines: &[String]) -> usize {
    let mut q = false;
    for (i, l) in lines.iter().enumerate() {
        for c in l.chars() {
            if c == '"' {
                q = !q;
            }
        }
        if !q {
            return i + 1;
        }
    }
    lines.len()
}

fn decode(lines: &[String], key_at: usize, n: usize) -> String {
    let mut s = String::new();
    for i in 0..n {
        let Some(v) = lines.get(i).and_then(|l| l.trim().parse::<i32>().ok()) else {
            return String::new();
        };
        let k = KEY[(key_at + i) % KEY.len()] as i32;
        let c = v ^ k;
        if !(32..127).contains(&c) {
            return String::new();
        }
        s.push(c as u8 as char);
    }
    s
}

impl AccountFile {
    /// The details, or None when the head of the file does not have the layout above.
    pub fn details(&self) -> Option<DetailsDto> {
        let head = &self.lines[..self.users_at.min(self.lines.len())];
        let span = header_span(head);
        if head.len() != span + AFTER {
            return None;
        }
        let text = head[..span].join("\n");
        let raw = raw_fields(&text);
        if raw.len() != FIELDS {
            return None;
        }
        let f: Vec<String> = raw.iter().map(|r| plain(r)).collect();
        let after = &head[span..];
        let num = |l: &String| l.trim().parse::<i32>().ok();
        Some(DetailsDto {
            name: f[1].clone(),
            address1: f[2].clone(),
            address2: f[3].clone(),
            city: f[4].clone(),
            state: f[5].clone(),
            zip: f[6].clone(),
            voice: f[7].clone(),
            fax: f[8].clone(),
            system_phone: f[9].clone(),
            system_url: f[10].clone(),
            port: f[11].clone(),
            port_c: f[12].clone(),
            email: f[13].clone(),
            web: f[14].clone(),
            folder: f[15].clone(),
            event_list: f[18].clone(),
            hardware: f[19].clone(),
            boot: f[20].clone(),
            voice_list: f[21].clone(),
            installed: f[22].clone(),
            notes: f[23].clone(),
            serial: decode(&after[0..8], 0, 8),
            rp_code: decode(&after[8..14], 8, 6),
            network_adapter: num(&after[45])?,
            non_secure: num(&after[46])?,
        })
    }

    /// Writes the editable fields that differ from what the file has. Does nothing when nothing changed, so an unedited
    /// account keeps its bytes; read-only fields in `d` are ignored.
    pub fn set_details(&mut self, d: &DetailsDto) -> Result<()> {
        let Some(cur) = self.details() else {
            return Ok(());
        };
        let editable = |x: &DetailsDto| {
            let mut y = x.clone();
            y.event_list.clear();
            y.hardware.clear();
            y.boot.clear();
            y.voice_list.clear();
            y.serial.clear();
            y.rp_code.clear();
            y
        };
        if editable(&cur) == editable(d) {
            return Ok(());
        }
        let texts: [(&str, &str, usize); 14] = [
            (LIMITS[0].0, &d.name, 1),
            (LIMITS[1].0, &d.address1, 2),
            (LIMITS[2].0, &d.address2, 3),
            (LIMITS[3].0, &d.city, 4),
            (LIMITS[4].0, &d.state, 5),
            (LIMITS[5].0, &d.zip, 6),
            (LIMITS[6].0, &d.folder, 15),
            (LIMITS[7].0, &d.voice, 7),
            (LIMITS[8].0, &d.fax, 8),
            (LIMITS[9].0, &d.email, 13),
            (LIMITS[10].0, &d.web, 14),
            (LIMITS[11].0, &d.notes, 23),
            (LIMITS[12].0, &d.system_phone, 9),
            (LIMITS[13].0, &d.system_url, 10),
        ];
        let head = &self.lines[..self.users_at];
        let span = header_span(head);
        let mut raw = raw_fields(&head[..span].join("\n"));
        for ((label, v, at), (_, max)) in texts.iter().zip(LIMITS.iter()) {
            let v = v.replace("\r\n", "\n").replace('\r', "\n");
            if v.contains('"') {
                return Err(Error::format(format!(
                    "{label}: the quote character (\") cannot be used in the account details."
                )));
            }
            if *at != 23 && v.contains('\n') {
                return Err(Error::format(format!(
                    "{label}: a line break is not allowed."
                )));
            }
            if v.chars().count() > *max {
                return Err(Error::format(format!(
                    "{label} is longer than {max} characters."
                )));
            }
            if v != plain(&raw[*at]) {
                raw[*at] = format!("\"{v}\"");
            }
        }
        for (label, v, at) in [("Port", &d.port, 11), ("C1M1 port(s)", &d.port_c, 12)] {
            if !(v.is_empty() || (v.len() <= 5 && v.bytes().all(|b| b.is_ascii_digit()))) {
                return Err(Error::format(format!(
                    "{label}: type a number of up to 5 digits."
                )));
            }
            if *v != plain(&raw[at]) {
                raw[at] = format!("\"{v}\"");
            }
        }
        if d.installed != cur.installed {
            if d.installed.contains('"') || d.installed.contains('\n') || d.installed.len() > 20 {
                return Err(Error::format("Date installed: not a valid date text."));
            }
            raw[22] = format!("\"{}\"", d.installed);
        }
        if !(0..=2).contains(&d.network_adapter) {
            return Err(Error::format("Network adapter must be 0, 1 or 2."));
        }
        if !(0..=3).contains(&d.non_secure) {
            return Err(Error::format("The non-secure flags must be 0 to 3."));
        }
        let mut new_head: Vec<String> = raw
            .join(",")
            .split('\n')
            .map(|l| l.trim_end_matches('\r').to_string())
            .collect();
        new_head.extend(head[span..span + 45].iter().cloned());
        new_head.push(if d.network_adapter == cur.network_adapter {
            head[span + 45].clone()
        } else {
            d.network_adapter.to_string()
        });
        new_head.push(if d.non_secure == cur.non_secure {
            head[span + 46].clone()
        } else {
            d.non_secure.to_string()
        });
        self.head = Some(new_head);
        Ok(())
    }
}
