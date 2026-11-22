//! The M1XEP's own settings ("M1XEP Setup" of ElkRP): the five setup blocks and the email slots, their layout, a decoded view,
//! validation and encoding of changes, and the read / write conversation with the module. See `rust/M1XEP-NOTES.md`.
//!
//! Derived from ElkRP's `XEPConfig.cs` (`Form_Receive`, `Form_Send`, `Show_Item`, `Form_Export`, `Form_Import`), `XEPFind.cs` and `M1Fns.SendReceive`.
//! The module is spoken to over the same framed session as the panel (opcode `0x75`): read `75 N`, write `75 80+N data`.
//! Everything here is checked only against the in-process simulator ([`XepSim`], used by `SimulatedPanel`), not against a real module.

use crate::elk_session::ElkSession;
use crate::error::{ElkError, Result};
use serde::{Deserialize, Serialize};
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

pub const XEP1_LEN: usize = 42;
pub const XEP2_LEN: usize = 644;
pub const XEP4_LEN: usize = 210;
pub const XEP5_LEN: usize = 532;
pub const XEP6_LEN: usize = 596;
pub const EMAIL_LEN: usize = 303;
pub const EMAIL_SLOTS: usize = 16;
pub const LOGINS: usize = 8;
pub const AUDIO_ZONES: usize = 18;
pub const AUDIO_SOURCES: usize = 12;
pub const REPORTS: usize = 8;
/// Lines the block takes in an account export file without any email: 42 + 644 + 210 + 532 + 596 values and the email count.
pub const FILE_FIXED_LINES: usize = XEP1_LEN + XEP2_LEN + XEP4_LEN + XEP5_LEN + XEP6_LEN + 1;
/// Firmware 1.3.19 as ElkRP compares it (`major * 65536 + minor * 256 + patch`): the layout this module reads and writes. Older modules use shorter
/// replies with a different layout, which this code does not handle.
pub const MIN_APP_VERSION: u32 = 66323;
/// Sub-commands of opcode 0x75 (read; a write adds 0x80).
pub const SUB_XEP1: u8 = 1;
pub const SUB_XEP2: u8 = 2;
pub const SUB_EMAIL: u8 = 3;
pub const SUB_XEP4: u8 = 4;
pub const SUB_VERSIONS: u8 = 6;
pub const SUB_XEP5: u8 = 7;
pub const SUB_XEP6: u8 = 8;
/// The ElkRP "Reboot" command. Documented for the notes only: nothing in this crate sends it.
pub const SUB_REBOOT: u8 = 0xAA;

/// The raw setup data of one M1XEP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XepBlocks {
    pub xep1: Vec<u8>,
    pub xep2: Vec<u8>,
    pub xep4: Vec<u8>,
    pub xep5: Vec<u8>,
    pub xep6: Vec<u8>,
    /// Sixteen slots; `None` = not defined (ElkRP treats a slot whose first byte is 0 as not defined).
    pub emails: Vec<Option<Vec<u8>>>,
}

impl Default for XepBlocks {
    fn default() -> Self {
        Self::blank()
    }
}

impl XepBlocks {
    /// All zero, no emails.
    pub fn blank() -> Self {
        XepBlocks { xep1: vec![0; XEP1_LEN], xep2: vec![0; XEP2_LEN], xep4: vec![0; XEP4_LEN], xep5: vec![0; XEP5_LEN], xep6: vec![0; XEP6_LEN], emails: vec![None; EMAIL_SLOTS] }
    }

    /// The block as ElkRP's `Form_Export` writes it: the five blocks one value per line, the number of emails, then per email its number (1-16) and its 303 values.
    pub fn to_lines(&self) -> Vec<String> {
        let mut out = Vec::with_capacity(FILE_FIXED_LINES + 304 * EMAIL_SLOTS);
        for b in [&self.xep1, &self.xep2, &self.xep4, &self.xep5, &self.xep6] {
            out.extend(b.iter().map(|v| v.to_string()));
        }
        let n = self.emails.iter().flatten().count();
        out.push(n.to_string());
        for (i, e) in self.emails.iter().enumerate() {
            if let Some(e) = e {
                out.push((i + 1).to_string());
                out.extend(e.iter().map(|v| v.to_string()));
            }
        }
        out
    }

    /// Reads the block from the lines of an export file (exactly the block: nothing may follow it). `None` when the shape does not fit.
    pub fn from_lines(l: &[String]) -> Option<XepBlocks> {
        if l.len() < FILE_FIXED_LINES {
            return None;
        }
        let mut at = 0;
        fn take(l: &[String], at: &mut usize, n: usize) -> Option<Vec<u8>> {
            let v: Option<Vec<u8>> = l[*at..*at + n].iter().map(|s| s.trim().parse::<u8>().ok()).collect();
            *at += n;
            v
        }
        let mut b = XepBlocks::blank();
        b.xep1 = take(l, &mut at, XEP1_LEN)?;
        b.xep2 = take(l, &mut at, XEP2_LEN)?;
        b.xep4 = take(l, &mut at, XEP4_LEN)?;
        b.xep5 = take(l, &mut at, XEP5_LEN)?;
        b.xep6 = take(l, &mut at, XEP6_LEN)?;
        let count: usize = l[at].trim().parse().ok()?;
        at += 1;
        if count > EMAIL_SLOTS || l.len() != FILE_FIXED_LINES + count * (EMAIL_LEN + 1) {
            return None;
        }
        for _ in 0..count {
            let num: usize = l[at].trim().parse().ok()?;
            at += 1;
            if !(1..=EMAIL_SLOTS).contains(&num) || b.emails[num - 1].is_some() {
                return None;
            }
            b.emails[num - 1] = Some(take(l, &mut at, EMAIL_LEN)?);
        }
        Some(b)
    }

    /// Upper-case hex of every block (for backup files).
    pub fn to_hex(&self) -> XepHex {
        XepHex {
            xep1: hex(&self.xep1),
            xep2: hex(&self.xep2),
            xep4: hex(&self.xep4),
            xep5: hex(&self.xep5),
            xep6: hex(&self.xep6),
            emails: self.emails.iter().map(|e| e.as_ref().map(|e| hex(e))).collect(),
        }
    }

    /// Inverse of [`XepBlocks::to_hex`], with the block sizes checked.
    pub fn from_hex(h: &XepHex) -> Result<XepBlocks> {
        let mut b = XepBlocks::blank();
        let get = |s: &str, n: usize, what: &str| -> Result<Vec<u8>> {
            let v = crate::error::hex_decode(s)?;
            if v.len() != n {
                return Err(ElkError::InvalidData(format!("{what} must be {n} bytes, not {}.", v.len())));
            }
            Ok(v)
        };
        b.xep1 = get(&h.xep1, XEP1_LEN, "xep1")?;
        b.xep2 = get(&h.xep2, XEP2_LEN, "xep2")?;
        b.xep4 = get(&h.xep4, XEP4_LEN, "xep4")?;
        b.xep5 = get(&h.xep5, XEP5_LEN, "xep5")?;
        b.xep6 = get(&h.xep6, XEP6_LEN, "xep6")?;
        if h.emails.len() != EMAIL_SLOTS {
            return Err(ElkError::InvalidData("emails must have 16 slots".into()));
        }
        for (i, e) in h.emails.iter().enumerate() {
            if let Some(e) = e {
                b.emails[i] = Some(get(e, EMAIL_LEN, "email")?);
            }
        }
        Ok(b)
    }
}

fn hex(b: &[u8]) -> String {
    crate::error::hex_upper(b)
}

/// Hex form of [`XepBlocks`] (the backup file format).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct XepHex {
    pub xep1: String,
    pub xep2: String,
    pub xep4: String,
    pub xep5: String,
    pub xep6: String,
    pub emails: Vec<Option<String>>,
}

// ---- decoded settings ---------------------------------------------------------------------------------------------------------------------------------

/// One user name / password pair of the Passwords tab.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Login {
    pub user: String,
    pub password: String,
}

/// One row of the Email tab.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EmailItem {
    /// 1-16.
    pub slot: u8,
    pub address: String,
    pub message: String,
}

/// One central station of the Central Station tab (read only here).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ReportCs {
    /// 1-8.
    pub cs: u8,
    /// 0 G.E. OH2000E, 1 DSC Sur-Gard, 2 DSC Sur-Gard (MAC).
    pub receiver_type: u8,
    pub receiver_name: String,
    pub address: String,
    pub port: u32,
    pub account: String,
    pub encrypt: bool,
    pub key: String,
    pub supervise: bool,
    pub heartbeat_seconds: u32,
    pub receiver: u32,
    pub line: u32,
    pub use_dnis: bool,
    pub dnis: String,
}

/// The settings of the M1XEP Setup window in ElkRP's terms. Network addresses are dotted text; ports are numbers (non-secure port 0 = disabled).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct XepSettings {
    // TCP/IP Settings
    pub name: String,
    pub dhcp: bool,
    pub ip: String,
    pub mask: String,
    pub gateway: String,
    pub non_secure_port: u32,
    pub secure_port: u32,
    pub dns_auto: bool,
    pub dns1: String,
    pub dns2: String,
    pub discovery: bool,
    // Passwords
    pub no_passwords: bool,
    pub logins: Vec<Login>,
    // Email
    pub smtp_server: String,
    pub smtp_port: u32,
    pub mail_from: String,
    pub mail_user: String,
    pub mail_password: String,
    pub emails: Vec<EmailItem>,
    // Dynamic DNS
    pub ddns_url: String,
    pub ddns_user: String,
    pub ddns_password: String,
    pub ddns_host: String,
    pub ddns_error_email: String,
    pub ddns_notify_ip_change: bool,
    // Time Server
    pub time_server: String,
    pub time_hour: u8,
    pub time_minute: u8,
    pub time_notify_errors: bool,
    /// Hours from UTC (read only here: ElkRP takes it from the Sunrise/Sunset screen).
    pub time_zone: f64,
    // Audio System
    pub audio_ip: String,
    pub audio_port: u32,
    pub audio_tcp: bool,
    pub audio_model: u8,
    pub audio_zone_names: Vec<String>,
    pub audio_source_names: Vec<String>,
    // M1Cloud
    pub m1_cloud: bool,
    // Central Station (read only here)
    pub reports: Vec<ReportCs>,
}

/// The receiver type names of ElkRP's Central Station tab.
pub const RECEIVER_TYPES: [&str; 3] = ["G.E. OH2000E", "DSC Sur-Gard", "DSC Sur-Gard (MAC)"];

fn str_at(b: &[u8], at: usize, len: usize) -> String {
    let s: String = b[at..at + len].iter().take_while(|c| **c != 0).map(|c| *c as char).collect();
    s.trim().to_string()
}

fn ip_at(b: &[u8], at: usize) -> String {
    format!("{}.{}.{}.{}", b[at], b[at + 1], b[at + 2], b[at + 3])
}

fn u16_at(b: &[u8], at: usize) -> u32 {
    b[at] as u32 * 256 + b[at + 1] as u32
}

fn bit(b: u8, n: usize) -> bool {
    b & (1 << n) != 0
}

fn hex_at(b: &[u8], at: usize, len: usize) -> String {
    hex(&b[at..at + len])
}

fn all_zero(b: &[u8]) -> bool {
    b.iter().all(|v| *v == 0)
}

impl XepSettings {
    /// Reads every field the way `Show_Item` / `PopulateReportScreen` do (current firmware layout).
    pub fn decode(b: &XepBlocks) -> XepSettings {
        let (x1, x2, x4, x5, x6) = (&b.xep1, &b.xep2, &b.xep4, &b.xep5, &b.xep6);
        let secure = u16_at(x1, 30);
        let smtp_port = u16_at(x2, 496);
        let mut s = XepSettings {
            name: str_at(x1, 0, 16),
            dhcp: bit(x1[40], 0),
            ip: ip_at(x1, 16),
            mask: ip_at(x1, 20),
            gateway: ip_at(x1, 24),
            non_secure_port: u16_at(x1, 28),
            // ElkRP shows 2601 when the module holds 0 and keeps it in its data
            secure_port: if secure == 0 { 2601 } else { secure },
            dns_auto: bit(x1[40], 3),
            dns1: ip_at(x1, 32),
            dns2: ip_at(x1, 36),
            discovery: bit(x1[40], 2),
            no_passwords: bit(x1[40], 5),
            smtp_server: str_at(x2, 448, 48),
            smtp_port: if smtp_port == 0 { 25 } else { smtp_port },
            mail_from: str_at(x2, 500, 48),
            mail_user: str_at(x2, 548, 48),
            mail_password: str_at(x2, 596, 48),
            ddns_url: str_at(x2, 256, 48),
            ddns_user: str_at(x2, 304, 48),
            ddns_password: str_at(x2, 352, 48),
            ddns_host: str_at(x2, 400, 48),
            ddns_error_email: str_at(x4, 104, 48),
            ddns_notify_ip_change: bit(x4[154], 0),
            time_server: str_at(x4, 156, 48),
            time_hour: x4[206],
            time_minute: x4[207],
            time_notify_errors: bit(x4[208], 0),
            time_zone: (x4[209] as f64 - 52.0) / 4.0,
            audio_ip: str_at(x5, 0, 48),
            audio_port: u16_at(x5, 48),
            // ElkRP always shows TCP and sets this bit when it loads its data
            audio_tcp: bit(x5[50], 0),
            audio_model: x5[51],
            m1_cloud: bit(x1[40], 4),
            ..Default::default()
        };
        for i in 0..LOGINS {
            s.logins.push(Login { user: str_at(x2, i * 32, 16), password: str_at(x2, i * 32 + 16, 16) });
        }
        for (i, slot) in b.emails.iter().enumerate() {
            let (address, message) = match slot {
                Some(e) => (str_at(e, 0, 48), str_at(e, 48, 255)),
                None => (String::new(), String::new()),
            };
            s.emails.push(EmailItem { slot: i as u8 + 1, address, message });
        }
        for i in 0..AUDIO_ZONES {
            s.audio_zone_names.push(str_at(x5, 52 + 16 * i, 16));
        }
        for i in 0..AUDIO_SOURCES {
            s.audio_source_names.push(str_at(x5, 340 + 16 * i, 16));
        }
        for i in 0..REPORTS {
            let rt = x6[384 + i];
            let acct_raw = &x6[392 + 5 * i..392 + 5 * i + 5];
            let account = if all_zero(acct_raw) {
                String::new()
            } else if rt == 0 {
                u16_at(x4, 80 + 2 * i).to_string()
            } else if rt == 1 {
                hex_at(x6, 392 + 5 * i + 3, 2)
            } else {
                hex(acct_raw)
            };
            let key = &x6[432 + 16 * i..432 + 16 * i + 16];
            let use_dnis = bit(x6[563], i);
            let dnis = if use_dnis { hex_at(x6, 564 + 4 * i, 4).trim_start_matches('0').to_string() } else { String::new() };
            s.reports.push(ReportCs {
                cs: i as u8 + 1,
                receiver_type: rt,
                receiver_name: RECEIVER_TYPES.get(rt as usize).copied().unwrap_or("Unknown").to_string(),
                address: str_at(x6, 48 * i, 48),
                port: u16_at(x4, 32 + 2 * i),
                account,
                encrypt: bit(x6[560], i),
                key: if all_zero(key) { String::new() } else { hex(key) },
                supervise: bit(x6[561], i),
                heartbeat_seconds: x4[96 + i] as u32,
                receiver: u16_at(x4, 48 + 2 * i),
                line: u16_at(x4, 64 + 2 * i),
                use_dnis,
                dnis,
            });
        }
        s
    }
}

// ---- changes ------------------------------------------------------------------------------------------------------------------------------------------

/// One changed field, for the diff the page shows before anything is sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct XepChange {
    /// The tab of ElkRP's window ("TCP/IP Settings", "Passwords", "Email", "Dynamic DNS", "Time Server", "Audio System").
    pub tab: String,
    /// ElkRP's wording of the field.
    pub field: String,
    pub old: String,
    pub new: String,
    /// True for the fields that decide how the module is reached or who may log in (IP, mask, gateway, DHCP, DNS, ports, user names and passwords).
    pub identity: bool,
}

fn put_str(b: &mut [u8], at: usize, len: usize, s: &str, what: &str) -> Result<()> {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() > len {
        return Err(ElkError::Argument(format!("{what}: at most {len} characters.")));
    }
    if let Some(c) = chars.iter().find(|c| !(' '..='~').contains(c)) {
        return Err(ElkError::Argument(format!("{what}: the character {c:?} is not allowed (printable ASCII only).")));
    }
    for (i, slot) in b[at..at + len].iter_mut().enumerate() {
        *slot = chars.get(i).map_or(0, |c| *c as u8);
    }
    Ok(())
}

fn put_u16(b: &mut [u8], at: usize, v: u32) {
    b[at] = (v / 256) as u8;
    b[at + 1] = (v & 0xFF) as u8;
}

fn put_bit(b: &mut u8, n: usize, on: bool) {
    if on {
        *b |= 1 << n;
    } else {
        *b &= !(1 << n);
    }
}

/// Parses dotted decimal as ElkRP's address boxes hold it (four bytes).
pub fn parse_ipv4(s: &str, what: &str) -> Result<[u8; 4]> {
    let ip: Ipv4Addr = s.trim().parse().map_err(|_| ElkError::Argument(format!("{what}: \"{s}\" is not an address like 192.168.1.20.")))?;
    Ok(ip.octets())
}

fn port_ok(v: u32, what: &str, zero_ok: bool) -> Result<()> {
    if v > 65535 || (v == 0 && !zero_ok) {
        return Err(ElkError::Argument(format!("{what}: the port must be between {} and 65535.", if zero_ok { 0 } else { 1 })));
    }
    Ok(())
}

fn is_url(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_alphabetic())
}

/// A subnet mask as ElkRP's `MySubMask_ValidateData` accepts it: not zero, ones then zeros.
pub fn mask_ok(m: [u8; 4]) -> bool {
    let n = u32::from_be_bytes(m);
    n != 0 && n & 1 == 0 && n.leading_ones() + n.trailing_zeros() == 32
}

impl XepSettings {
    /// The checks ElkRP applies when the window is closed or sent, on the values as they would be written (only called for what changes).
    pub fn validate(&self) -> Result<()> {
        if !self.dhcp {
            let (ip, mask, gw) = (parse_ipv4(&self.ip, "IP Address")?, parse_ipv4(&self.mask, "Subnet Mask")?, parse_ipv4(&self.gateway, "Default Gateway")?);
            if !mask_ok(mask) {
                return Err(ElkError::Argument("Subnet Mask: it should be of the form 255.255.255.0 or 255.255.0.0.".into()));
            }
            if u32::from_be_bytes(gw) & 0x7F00_0000 == 0 && gw[1..].iter().all(|v| *v == 0) {
                return Err(ElkError::Argument("Default Gateway: it may not be 0.0.0.0.".into()));
            }
            let m = u32::from_be_bytes(mask);
            if u32::from_be_bytes(ip) & m != u32::from_be_bytes(gw) & m {
                return Err(ElkError::Argument("The Static IP Address and Default Gateway are not in the same subnet as defined by the Subnet Mask.".into()));
            }
        }
        port_ok(self.secure_port, "Secure Port", false)?;
        port_ok(self.non_secure_port, "Non-secure port", true)?;
        if self.non_secure_port == self.secure_port {
            return Err(ElkError::Argument("The Secure and Non-secure ports may not be set to the same number.".into()));
        }
        Ok(())
    }

    /// Applies the differences between `old` (what the base blocks decode to) and `new` onto a copy of `base`. Only changed fields are encoded, every other
    /// byte (including bytes this code does not know) stays as it was. Returns the new blocks and the list of changes.
    pub fn apply(base: &XepBlocks, old: &XepSettings, new: &XepSettings) -> Result<(XepBlocks, Vec<XepChange>)> {
        if new.logins.len() != LOGINS || new.emails.len() != EMAIL_SLOTS || new.audio_zone_names.len() != AUDIO_ZONES || new.audio_source_names.len() != AUDIO_SOURCES {
            return Err(ElkError::Argument("The settings are incomplete: send back every field that was read (8 logins, 16 emails, 18 audio zones, 12 audio sources).".into()));
        }
        if new.reports != old.reports {
            return Err(ElkError::Argument("The Central Station settings are read only here; change them in ElkRP.".into()));
        }
        if new.m1_cloud != old.m1_cloud {
            return Err(ElkError::Argument("The M1Cloud setting is read only here; change it in ElkRP.".into()));
        }
        if new.time_zone != old.time_zone {
            return Err(ElkError::Argument("The local time zone cannot be changed here (ElkRP takes it from the Sunrise/Sunset screen).".into()));
        }
        if new.audio_tcp != old.audio_tcp {
            return Err(ElkError::Argument("The audio transport is read only here.".into()));
        }
        let mut b = base.clone();
        let mut changes: Vec<XepChange> = Vec::new();
        let mut note = |tab: &str, field: &str, o: String, n: String, identity: bool| {
            changes.push(XepChange { tab: tab.into(), field: field.into(), old: o, new: n, identity });
        };
        const NET: &str = "TCP/IP Settings";
        if new.name != old.name {
            put_str(&mut b.xep1, 0, 16, &new.name, "Device Name")?;
            note(NET, "Device Name", old.name.clone(), new.name.clone(), false);
        }
        let net_changed = new.dhcp != old.dhcp || (!new.dhcp && (new.ip != old.ip || new.mask != old.mask || new.gateway != old.gateway));
        if net_changed
            || new.dns_auto != old.dns_auto
            || new.dns1 != old.dns1
            || new.dns2 != old.dns2
            || new.non_secure_port != old.non_secure_port
            || new.secure_port != old.secure_port
        {
            new.validate()?;
        }
        if new.dhcp != old.dhcp {
            put_bit(&mut b.xep1[40], 0, new.dhcp);
            let w = |d: bool| if d { "Assigned an IP address via DHCP" } else { "Use a static IP address" };
            note(NET, "IP address source", w(old.dhcp).into(), w(new.dhcp).into(), true);
        }
        for (field, o, n, at) in [("IP Address", &old.ip, &new.ip, 16usize), ("Subnet Mask", &old.mask, &new.mask, 20), ("Default Gateway", &old.gateway, &new.gateway, 24)] {
            if o != n {
                if new.dhcp {
                    // ElkRP keeps the (unused) static values; changing them while DHCP is on has no effect, so refuse rather than write unseen bytes
                    return Err(ElkError::Argument(format!("{field}: turn DHCP off first (the static values are not used while DHCP is on).")));
                }
                b.xep1[at..at + 4].copy_from_slice(&parse_ipv4(n, field)?);
                note(NET, field, o.clone(), n.clone(), true);
            }
        }
        if new.non_secure_port != old.non_secure_port {
            put_u16(&mut b.xep1, 28, new.non_secure_port);
            let w = |p: u32| if p == 0 { "disabled".to_string() } else { p.to_string() };
            note(NET, "Non-secure port", w(old.non_secure_port), w(new.non_secure_port), true);
        }
        if new.secure_port != old.secure_port {
            put_u16(&mut b.xep1, 30, new.secure_port);
            note(NET, "Secure Port", old.secure_port.to_string(), new.secure_port.to_string(), true);
        }
        if new.dns_auto != old.dns_auto {
            put_bit(&mut b.xep1[40], 3, new.dns_auto);
            let w = |d: bool| if d { "Obtain DNS automatically" } else { "Specify DNS Addresses to use" };
            note(NET, "DNS", w(old.dns_auto).into(), w(new.dns_auto).into(), true);
        }
        for (field, o, n, at) in [("DNS Primary", &old.dns1, &new.dns1, 32usize), ("DNS Secondary", &old.dns2, &new.dns2, 36)] {
            if o != n {
                b.xep1[at..at + 4].copy_from_slice(&parse_ipv4(n, field)?);
                note(NET, field, o.clone(), n.clone(), true);
            }
        }
        if new.discovery != old.discovery {
            put_bit(&mut b.xep1[40], 2, new.discovery);
            note(NET, "Enable Discovery of M1XEP for AMX and Control4 systems", old.discovery.to_string(), new.discovery.to_string(), false);
        }
        const PW: &str = "Passwords";
        if new.no_passwords != old.no_passwords {
            put_bit(&mut b.xep1[40], 5, new.no_passwords);
            note(PW, "Disable username/passwords", old.no_passwords.to_string(), new.no_passwords.to_string(), true);
        }
        for i in 0..LOGINS {
            if new.logins[i].user != old.logins[i].user {
                put_str(&mut b.xep2, i * 32, 16, &new.logins[i].user, "User name")?;
                note(PW, &format!("User name {}", i + 1), old.logins[i].user.clone(), new.logins[i].user.clone(), true);
            }
            if new.logins[i].password != old.logins[i].password {
                put_str(&mut b.xep2, i * 32 + 16, 16, &new.logins[i].password, "Password")?;
                note(PW, &format!("Password {}", i + 1), mask_secret(&old.logins[i].password), mask_secret(&new.logins[i].password), true);
            }
        }
        const EM: &str = "Email";
        let text_fields: [(&str, &String, &String, usize, bool); 4] = [
            ("Server URL/IP Address", &old.smtp_server, &new.smtp_server, 448, false),
            ("\"FROM\" email address", &old.mail_from, &new.mail_from, 500, false),
            ("Username", &old.mail_user, &new.mail_user, 548, false),
            ("Password", &old.mail_password, &new.mail_password, 596, true),
        ];
        for (field, o, n, at, secret) in text_fields {
            if o != n {
                put_str(&mut b.xep2, at, 48, n, field)?;
                note(EM, field, if secret { mask_secret(o) } else { o.clone() }, if secret { mask_secret(n) } else { n.clone() }, false);
            }
        }
        if new.smtp_port != old.smtp_port {
            port_ok(new.smtp_port, "Port", false)?;
            put_u16(&mut b.xep2, 496, new.smtp_port);
            note(EM, "Port", old.smtp_port.to_string(), new.smtp_port.to_string(), false);
        }
        for i in 0..EMAIL_SLOTS {
            let (o, n) = (&old.emails[i], &new.emails[i]);
            if o.address == n.address && o.message == n.message {
                continue;
            }
            let mut e = base.emails[i].clone().unwrap_or_else(|| vec![0; EMAIL_LEN]);
            put_str(&mut e, 0, 48, &n.address, "Email Address")?;
            put_str(&mut e, 48, 255, &n.message, "Message")?;
            // a slot is "defined" when its first byte is not 0: an empty address with a message would vanish on the module
            if n.address.is_empty() && !n.message.is_empty() {
                return Err(ElkError::Argument(format!("Email {}: a message needs an email address.", i + 1)));
            }
            b.emails[i] = if n.address.is_empty() { None } else { Some(e) };
            if o.address != n.address {
                note(EM, &format!("Email {} address", i + 1), o.address.clone(), n.address.clone(), false);
            }
            if o.message != n.message {
                note(EM, &format!("Email {} message", i + 1), o.message.clone(), n.message.clone(), false);
            }
        }
        const DD: &str = "Dynamic DNS";
        let ddns: [(&str, &String, &String, usize, bool); 4] = [
            ("URL or IP Address of Dynamic DNS Provider", &old.ddns_url, &new.ddns_url, 256, false),
            ("User name", &old.ddns_user, &new.ddns_user, 304, false),
            ("Password", &old.ddns_password, &new.ddns_password, 352, true),
            ("Host name", &old.ddns_host, &new.ddns_host, 400, false),
        ];
        for (field, o, n, at, secret) in ddns {
            if o != n {
                put_str(&mut b.xep2, at, 48, n, field)?;
                note(DD, field, if secret { mask_secret(o) } else { o.clone() }, if secret { mask_secret(n) } else { n.clone() }, false);
            }
        }
        if new.ddns_error_email != old.ddns_error_email {
            put_str(&mut b.xep4, 104, 48, &new.ddns_error_email, "Email address to send error reports")?;
            note(DD, "Email address to send error reports", old.ddns_error_email.clone(), new.ddns_error_email.clone(), false);
        }
        if new.ddns_notify_ip_change != old.ddns_notify_ip_change {
            put_bit(&mut b.xep4[154], 0, new.ddns_notify_ip_change);
            note(DD, "Send a notice whenever the public IP address changes", old.ddns_notify_ip_change.to_string(), new.ddns_notify_ip_change.to_string(), false);
        }
        const TS: &str = "Time Server";
        if new.time_server != old.time_server {
            put_str(&mut b.xep4, 156, 48, &new.time_server, "URL or IP Address of Time Server")?;
            note(TS, "URL or IP Address of Time Server", old.time_server.clone(), new.time_server.clone(), false);
        }
        if new.time_hour != old.time_hour || new.time_minute != old.time_minute {
            if new.time_hour > 23 || new.time_minute > 59 {
                return Err(ElkError::Argument("Time of day to perform update: hour 0-23, minute 0-59.".into()));
            }
            b.xep4[206] = new.time_hour;
            b.xep4[207] = new.time_minute;
            note(TS, "Time of day to perform update", format!("{:02}:{:02}", old.time_hour, old.time_minute), format!("{:02}:{:02}", new.time_hour, new.time_minute), false);
        }
        if new.time_notify_errors != old.time_notify_errors {
            put_bit(&mut b.xep4[208], 0, new.time_notify_errors);
            note(TS, "Send a notice when an error is encountered updating the time", old.time_notify_errors.to_string(), new.time_notify_errors.to_string(), false);
        }
        const AU: &str = "Audio System";
        if new.audio_ip != old.audio_ip {
            put_str(&mut b.xep5, 0, 48, &new.audio_ip, "Audio Controller IP Address or URL")?;
            put_bit(&mut b.xep5[50], 1, is_url(new.audio_ip.trim()));
            note(AU, "Audio Controller IP Address or URL", old.audio_ip.clone(), new.audio_ip.clone(), false);
        }
        if new.audio_port != old.audio_port {
            port_ok(new.audio_port, "TCP Port", true)?;
            put_u16(&mut b.xep5, 48, new.audio_port);
            note(AU, "TCP Port", old.audio_port.to_string(), new.audio_port.to_string(), false);
        }
        if new.audio_model != old.audio_model {
            b.xep5[51] = new.audio_model;
            note(AU, "Audio Controller Manufacturer/Model", old.audio_model.to_string(), new.audio_model.to_string(), false);
        }
        for i in 0..AUDIO_ZONES {
            if new.audio_zone_names[i] != old.audio_zone_names[i] {
                put_str(&mut b.xep5, 52 + 16 * i, 16, &new.audio_zone_names[i], "Audio Zone Name")?;
                note(AU, &format!("Audio Zone {} name", i + 1), old.audio_zone_names[i].clone(), new.audio_zone_names[i].clone(), false);
            }
        }
        for i in 0..AUDIO_SOURCES {
            if new.audio_source_names[i] != old.audio_source_names[i] {
                put_str(&mut b.xep5, 340 + 16 * i, 16, &new.audio_source_names[i], "Audio Source Name")?;
                note(AU, &format!("Audio Source {} name", i + 1), old.audio_source_names[i].clone(), new.audio_source_names[i].clone(), false);
            }
        }
        Ok((b, changes))
    }
}

fn mask_secret(s: &str) -> String {
    if s.is_empty() {
        String::new()
    } else {
        "*".repeat(s.chars().count().min(16))
    }
}

// ---- the conversation with the module -------------------------------------------------------------------------------------------------------------------

/// What `75 06` says about the module (the same reply the module scan decodes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct XepInfo {
    pub hardware: String,
    pub boot: String,
    pub firmware: String,
    /// `major * 65536 + minor * 256 + patch`, ElkRP's `XEP_XIP_AppVer`.
    pub app_version: u32,
}

/// Read and write of the module's setup over an open session.
pub struct XepModule;

fn ver(v: u32) -> String {
    format!("{}.{}.{}", (v >> 16) & 0xFF, (v >> 8) & 0xFF, v & 0xFF)
}

impl XepModule {
    fn expect(s: &mut ElkSession, sub: u8, idx: Option<u8>, min_len: usize) -> Result<Vec<u8>> {
        for _ in 0..8 {
            let r = s.receive(0x75)?;
            let ok = r.get(1) == Some(&sub) && idx.is_none_or(|i| r.get(2) == Some(&i));
            if ok {
                if r.len() < min_len {
                    return Err(ElkError::InvalidData(format!("The M1XEP answered with {} bytes, expected at least {min_len}.", r.len())));
                }
                return Ok(r);
            }
        }
        Err(ElkError::Timeout(format!("The M1XEP did not answer sub-command {sub:#04X}.")))
    }

    /// `75 06`: versions of the module. Read only.
    pub fn info(s: &mut ElkSession) -> Result<XepInfo> {
        s.send(&[0x75, SUB_VERSIONS])?;
        let r = Self::expect(s, SUB_VERSIONS, None, 16)?;
        let app = (r[13] as u32) << 16 | (r[14] as u32) << 8 | r[15] as u32;
        Ok(XepInfo { hardware: format!("{}.{}", r[8], r[9]), boot: ver((r[10] as u32) << 16 | (r[11] as u32) << 8 | r[12] as u32), firmware: ver(app), app_version: app })
    }

    /// `75 N` for N = 1, 2, 4, 7, 8 and `75 03 idx` for the 16 email slots: what ElkRP's "Receive" button does. Read only; nothing is stored on the module.
    pub fn read(s: &mut ElkSession) -> Result<(XepInfo, XepBlocks)> {
        let info = Self::info(s)?;
        if info.app_version < MIN_APP_VERSION {
            return Err(ElkError::InvalidOperation(format!("The M1XEP firmware is {}; this editor handles 1.3.19 and later (older modules use another layout).", info.firmware)));
        }
        let mut b = XepBlocks::blank();
        for (sub, len, slot) in [(SUB_XEP1, XEP1_LEN, 0usize), (SUB_XEP2, XEP2_LEN, 1), (SUB_XEP4, XEP4_LEN, 2), (SUB_XEP5, XEP5_LEN, 3), (SUB_XEP6, XEP6_LEN, 4)] {
            s.send(&[0x75, sub])?;
            let r = Self::expect(s, sub, None, 2 + len)?;
            let data = r[2..2 + len].to_vec();
            match slot {
                0 => b.xep1 = data,
                1 => b.xep2 = data,
                2 => b.xep4 = data,
                3 => b.xep5 = data,
                _ => b.xep6 = data,
            }
        }
        for (i, slot) in b.emails.iter_mut().enumerate() {
            s.send(&[0x75, SUB_EMAIL, i as u8])?;
            let r = Self::expect(s, SUB_EMAIL, Some(i as u8), 3 + EMAIL_LEN)?;
            if r[3] > 0 {
                *slot = Some(r[3..3 + EMAIL_LEN].to_vec());
            }
        }
        Ok((info, b))
    }

    /// Sends the blocks that differ between `from` (what the module holds) and `to`, in ElkRP's order (1, 2, 4, 5, 6, then emails). Returns what was sent.
    /// `timeout` is how long each block may take to be acknowledged (ElkRP warns it "may take up to a minute"; use 60 s). Does not reboot and does not send the daylight-saving block (`75 8A`); the module applies the new setup after a reboot.
    pub fn write(s: &mut ElkSession, from: &XepBlocks, to: &XepBlocks, timeout: Duration) -> Result<Vec<String>> {
        let old_timeout = s.timeout;
        s.timeout = timeout;
        let r = Self::write_inner(s, from, to);
        s.timeout = old_timeout;
        r
    }

    fn write_inner(s: &mut ElkSession, from: &XepBlocks, to: &XepBlocks) -> Result<Vec<String>> {
        let mut sent = Vec::new();
        for (sub, name, a, b) in [
            (SUB_XEP1, "network settings and flags (block 1)", &from.xep1, &to.xep1),
            (SUB_XEP2, "logins, email server, dynamic DNS (block 2)", &from.xep2, &to.xep2),
            (SUB_XEP4, "ports, time server and report settings (block 4)", &from.xep4, &to.xep4),
            (SUB_XEP5, "audio system (block 5)", &from.xep5, &to.xep5),
            (SUB_XEP6, "central station addresses (block 6)", &from.xep6, &to.xep6),
        ] {
            if a == b {
                continue;
            }
            let mut p = vec![0x75, 0x80 | sub];
            p.extend_from_slice(b);
            s.send(&p)?;
            Self::expect(s, 0x80 | sub, None, 2)?;
            sent.push(name.to_string());
        }
        for i in 0..EMAIL_SLOTS {
            if from.emails[i] == to.emails[i] {
                continue;
            }
            let mut p = vec![0x75, 0x80 | SUB_EMAIL, i as u8];
            match &to.emails[i] {
                Some(e) => p.extend_from_slice(e),
                None => p.extend(std::iter::repeat_n(0u8, EMAIL_LEN)),
            }
            s.send(&p)?;
            Self::expect(s, 0x80 | SUB_EMAIL, Some(i as u8), 3)?;
            sent.push(format!("email {}", i + 1));
        }
        Ok(sent)
    }
}

// ---- discovery (ElkRP "Find") ----------------------------------------------------------------------------------------------------------------------------

/// A module that answered the "XEPID" broadcast.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundXep {
    pub name: String,
    pub mac: String,
    pub ip: String,
    pub secure_port: u32,
}

/// Decodes one discovery reply (at least 33 bytes starting with `M1XEP`).
pub fn parse_discovery_reply(d: &[u8]) -> Option<FoundXep> {
    if d.len() < 33 || &d[..5] != b"M1XEP" {
        return None;
    }
    Some(FoundXep { name: str_at(d, 17, 16), mac: d[5..11].iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join("-"), ip: ip_at(d, 11), secure_port: u16_at(d, 15) })
}

/// The UDP port ElkRP uses for discovery.
pub const DISCOVERY_PORT: u16 = 2362;

/// Sends the ASCII request `XEPID` twice, one second apart (like ElkRP), to `target` and collects the replies for `wait`. `target` is the module's own
/// address (unicast) or the broadcast address. Read only: the request changes nothing on the module.
pub fn discover(target: SocketAddr, broadcast: bool, wait: Duration) -> std::io::Result<Vec<FoundXep>> {
    // ElkRP binds its own port 2362; any local port works if the module answers to the sender, so fall back to a free one.
    let sock = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, DISCOVERY_PORT)).or_else(|_| UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)))?;
    sock.set_broadcast(broadcast)?;
    let end = Instant::now() + wait;
    let mut found: Vec<FoundXep> = Vec::new();
    let mut next_send = Instant::now();
    let mut sends = 0;
    let mut buf = [0u8; 512];
    while Instant::now() < end {
        if sends < 2 && Instant::now() >= next_send {
            sock.send_to(b"XEPID", target)?;
            sends += 1;
            next_send = Instant::now() + Duration::from_millis(1000);
        }
        sock.set_read_timeout(Some(Duration::from_millis(50)))?;
        match sock.recv_from(&mut buf) {
            Ok((n, _)) => {
                if let Some(f) = parse_discovery_reply(&buf[..n]) {
                    if !found.iter().any(|x| x.mac == f.mac && x.ip == f.ip) {
                        found.push(f);
                    }
                }
            }
            // WouldBlock / TimedOut: nothing yet. ConnectionReset / ConnectionRefused: Windows reports the ICMP "port unreachable" of an earlier send here.
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut | std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionRefused
                ) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(found)
}

// ---- the simulated module -------------------------------------------------------------------------------------------------------------------------------

/// A software M1XEP for tests: answers `75 N` reads and `75 80+N` writes from [`XepBlocks`]. It follows ElkRP's code only (see the module comment).
pub struct XepSim {
    /// Off by default so the panel simulator keeps answering nothing to opcode 0x75 (the recorded module-scan cases rely on that).
    pub enabled: bool,
    pub app_version: u32,
    pub blocks: XepBlocks,
    /// Every write request received, in order (payload as sent).
    pub writes: Vec<Vec<u8>>,
    /// When set, writes of this sub-command (0x81, 0x82, ...) are not answered.
    pub ignore_write: Option<u8>,
}

impl Default for XepSim {
    fn default() -> Self {
        XepSim { enabled: false, app_version: 0x01_0314, blocks: XepBlocks::blank(), writes: Vec::new(), ignore_write: None }
    }
}

impl XepSim {
    /// The reply to one `0x75` request, or `None` for no reply.
    pub fn handle(&mut self, p: &[u8]) -> Option<Vec<u8>> {
        if !self.enabled || p.len() < 2 || p[0] != 0x75 {
            return None;
        }
        let sub = p[1];
        if sub >= 0x80 {
            self.writes.push(p.to_vec());
            if self.ignore_write == Some(sub) {
                return None;
            }
        }
        let mut r = vec![0x75, sub];
        match sub {
            SUB_VERSIONS => {
                r.resize(16, 0);
                r[8] = 1;
                r[9] = 2;
                r[10] = 1;
                r[11] = 0;
                r[12] = 3;
                r[13] = (self.app_version >> 16) as u8;
                r[14] = (self.app_version >> 8) as u8;
                r[15] = self.app_version as u8;
            }
            SUB_XEP1 => r.extend_from_slice(&self.blocks.xep1),
            SUB_XEP2 => r.extend_from_slice(&self.blocks.xep2),
            SUB_XEP4 => r.extend_from_slice(&self.blocks.xep4),
            SUB_XEP5 => r.extend_from_slice(&self.blocks.xep5),
            SUB_XEP6 => r.extend_from_slice(&self.blocks.xep6),
            SUB_EMAIL if p.len() >= 3 && (p[2] as usize) < EMAIL_SLOTS => {
                r.push(p[2]);
                match &self.blocks.emails[p[2] as usize] {
                    Some(e) => r.extend_from_slice(e),
                    None => r.extend(std::iter::repeat_n(0u8, EMAIL_LEN)),
                }
            }
            s if s == 0x80 | SUB_XEP1 && p.len() == 2 + XEP1_LEN => self.blocks.xep1 = p[2..].to_vec(),
            s if s == 0x80 | SUB_XEP2 && p.len() == 2 + XEP2_LEN => self.blocks.xep2 = p[2..].to_vec(),
            s if s == 0x80 | SUB_XEP4 && p.len() == 2 + XEP4_LEN => self.blocks.xep4 = p[2..].to_vec(),
            s if s == 0x80 | SUB_XEP5 && p.len() == 2 + XEP5_LEN => self.blocks.xep5 = p[2..].to_vec(),
            s if s == 0x80 | SUB_XEP6 && p.len() == 2 + XEP6_LEN => self.blocks.xep6 = p[2..].to_vec(),
            s if s == 0x80 | SUB_EMAIL && p.len() == 3 + EMAIL_LEN && (p[2] as usize) < EMAIL_SLOTS => {
                self.blocks.emails[p[2] as usize] = if p[3] > 0 { Some(p[3..].to_vec()) } else { None };
                r.push(p[2]);
            }
            _ => return None,
        }
        Some(r)
    }
}
