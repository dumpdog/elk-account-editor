//! Port of PanelSync.cs: `PanelChange`, `SyncResult`, `PanelConnection` and `PanelSync` (diff, pull, apply and the transfer estimates).

use crate::auto_sync;
use crate::error::{PanelError, Result};
use elk_account::{AccountFile, Section as FileSection};
use elk_core::{DefaultRecords, ElkError, ElkSession, KeypadRecord, PanelProgress};
use std::collections::{BTreeMap, HashSet};
use std::time::Duration;

/// One record that differs between an account file and the panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelChange {
    /// Section label (`zone`, `user report codes`, `zone (reset)`, ...).
    pub label: String,
    /// Item number.
    pub number: i32,
    /// Panel section id.
    pub section: u8,
    /// The bytes to write.
    pub data: Vec<u8>,
    /// A keypad (two transfers).
    pub is_keypad: bool,
    /// Part (wireless blocks).
    pub part: u8,
}

/// Result of [`PanelSync::diff`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncResult {
    /// The records that differ.
    pub changes: Vec<PanelChange>,
    /// Per section label (ordered by label): records already identical, records that differ.
    pub counts: BTreeMap<String, (i32, i32)>,
    /// Records that have no panel item (`user 201`).
    pub skipped: Vec<String>,
}

/// How to reach a panel (`PanelConnection.Settings`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Host name or address.
    pub host: String,
    /// TCP port (an `int` like the C# record: values outside 0..=65535 fail when connecting).
    pub port: i32,
    /// RP access code.
    pub code: String,
    /// Plain (non-secure) network port.
    pub plain: bool,
    /// MAC address for the AES port.
    pub mac: Option<String>,
    /// Passphrase for the AES port.
    pub passphrase: Option<String>,
    /// TLS port (M1XEP secure).
    pub tls: bool,
    /// Panel serial (8 hex digits) for the TLS port.
    pub serial: Option<String>,
}

/// Number of connection attempts and the pause between them used by the CLI and the editor.
pub const DEFAULT_ATTEMPTS: u32 = 6;
/// Pause between connection attempts.
pub const DEFAULT_RETRY_DELAY: Duration = Duration::from_millis(5000);

/// Connects with the same retry the CLI uses (the panel keeps a previous session open for a while after a write).
pub struct PanelConnection;

/// Turns an I/O error text into the .NET `SocketException` wording: the OS text without Rust's ` (os error N)` suffix (on Windows this equals .NET's text,
/// for example `No connection could be made because the target machine actively refused it.`).
fn socket_error(e: ElkError) -> ElkError {
    match e {
        ElkError::Io(io) => {
            let text = io.to_string();
            let base = match text.rfind(" (os error ") {
                Some(i) => text[..i].to_string(),
                None => text,
            };
            ElkError::Io(std::io::Error::new(io.kind(), base))
        }
        other => other,
    }
}

impl PanelConnection {
    /// `OpenAsync`: connect and log in, retrying `attempts` times with `delay` between them on a timeout or an I/O error. `log` gets one line per retry.
    pub fn open(c: &Settings, log: &mut dyn FnMut(&str), attempts: u32, delay: Duration) -> std::result::Result<ElkSession, ElkError> {
        let mut attempt = 1;
        loop {
            let mut s = if c.plain {
                ElkSession::plain()
            } else if c.tls {
                ElkSession::tls(c.serial.as_deref().ok_or_else(|| ElkError::Argument("The panel serial number is needed for the secure port".into()))?)?
            } else {
                ElkSession::new(c.mac.as_deref().ok_or_else(|| ElkError::Argument("MAC address is needed for the AES port".into()))?, c.passphrase.as_deref().unwrap_or(""))?
            };
            let connected = (|| -> std::result::Result<(), (ElkError, bool)> {
                if !(0..=65535).contains(&c.port) {
                    // .NET: an IP address literal fails in the endpoint with the stock text, a host name in the port check with the long one
                    let literal = c.host.parse::<std::net::IpAddr>().is_ok();
                    let m = if literal {
                        "Specified argument was out of the range of valid values. (Parameter 'port')".to_string()
                    } else if c.port > 65535 {
                        format!("port ('{0}') must be less than or equal to '65535'. (Parameter 'port')\r\nActual value was {0}.", c.port)
                    } else {
                        format!("port ('{0}') must be greater than or equal to '0'. (Parameter 'port')\r\nActual value was {0}.", c.port)
                    };
                    return Err((ElkError::OutOfRange(m), true));
                }
                s.connect(&c.host, c.port as u16).map_err(|e| (socket_error(e), true))?;
                s.login(&c.code).map_err(|e| (e, false))?;
                Ok(())
            })();
            match connected {
                Ok(()) => return Ok(s),
                Err((e, at_connect)) => {
                    let retry = attempt < attempts && matches!(e, ElkError::Timeout(_) | ElkError::Io(_));
                    if !retry {
                        return Err(e);
                    }
                    let type_name = match &e {
                        ElkError::Timeout(_) => "TimeoutException",
                        _ if at_connect => "SocketException",
                        _ => "IOException",
                    };
                    log(&format!("login attempt {attempt} failed ({type_name}); retrying in {} s", delay.as_millis() / 1000));
                    std::thread::sleep(delay);
                    attempt += 1;
                }
            }
        }
    }
}

fn comm_count(s: &FileSection) -> i32 {
    s.entries.len() as i32
}

fn upto(s: &FileSection, max: i32) -> i32 {
    s.entries.iter().filter(|e| e.number <= max).count() as i32
}

fn missing(s: &FileSection, from: i32, to: i32) -> i32 {
    (to - from + 1) - s.entries.iter().filter(|e| e.number >= from && e.number <= to).count() as i32
}

fn wireless_len(f: &AccountFile) -> i32 {
    f.wireless().ok().flatten().map_or(0, |w| w.len() as i32)
}

fn globals_48(f: &AccountFile) -> Option<Vec<u8>> {
    f.globals().ok().flatten().filter(|g| g.len() == 48)
}

/// `Encoding.ASCII.GetString(b, off, len).TrimEnd('\0', ' ')` (a byte above 127 decodes to `?`).
fn name_at(b: &[u8], off: usize, len: usize) -> Result<String> {
    if b.len() < off + len {
        return Err(PanelError::other("Index and length must refer to a location within the string. (Parameter 'bytes')"));
    }
    let s: String = b[off..off + len].iter().map(|&c| if c < 128 { c as char } else { '?' }).collect();
    Ok(s.trim_end_matches(['\0', ' ']).to_string())
}

fn need(b: &[u8], len: usize) -> Result<()> {
    if b.len() < len {
        Err(PanelError::other("Index was outside the bounds of the array."))
    } else {
        Ok(())
    }
}

/// Compares an ElkRP account file with the panel record by record and writes what differs.
/// Only records present in the file are touched. Globals and dawn/dusk are not sent; users and user report codes above 199 have no panel item.
pub struct PanelSync;

impl PanelSync {
    /// How many records or blocks [`PanelSync::diff`] will read (a keypad is two transfers). Used for the progress bar.
    pub fn estimate_diff_transfers(f: &AccountFile, reset_missing: bool) -> i32 {
        let mut n =
            upto(&f.users, 199) + upto(&f.areas, 8) + upto(&f.zones, 208) + 2 * comm_count(&f.keypads) + upto(&f.phones, 8) + wireless_len(f) + i32::from(globals_48(f).is_some());
        if f.has_comm() {
            n += upto(&f.area_rcs, 8) + upto(&f.user_rcs, 199) + 1 + upto(&f.zone_rcs, 208) + 1;
        }
        if reset_missing {
            n += missing(&f.zones, 1, 208) + missing(&f.users, 2, 199) + missing(&f.areas, 2, 8) + missing(&f.phones, 1, 8);
            if f.has_comm() {
                n += missing(&f.area_rcs, 2, 8) + missing(&f.user_rcs, 2, 199) + missing(&f.zone_rcs, 1, 208);
            }
            n += 2 * missing(&f.keypads, 2, 16);
        }
        n
    }

    /// The transfers [`PanelSync::pull`] makes before it reads the report codes (their number is added when the records are known).
    pub fn estimate_pull_transfers(f: &AccountFile) -> i32 {
        wireless_len(f) + i32::from(f.globals().ok().flatten().is_some()) + 208 + 2 * 16 + 8 + 199 + if f.has_phone_section() { 8 } else { 0 }
    }

    /// The transfers needed to write these changes (a keypad is two).
    pub fn write_transfers(changes: &[PanelChange]) -> i32 {
        changes.iter().filter(|c| !auto_sync::is_auto_change(c)).map(|c| if c.is_keypad { 2 } else { 1 }).sum::<i32>() + auto_sync::write_transfers(changes)
    }

    #[allow(clippy::too_many_arguments)] // the C# local function `Block` takes these too
    fn block(s: &mut ElkSession, r: &mut SyncResult, label: &str, sec: u8, num: i32, want: &[u8], max: i32, part: u8) -> Result<()> {
        if num > max {
            r.skipped.push(format!("{label} {num}"));
            return Ok(());
        }
        let have = s.read_block(sec, num as u8, part)?;
        let c = r.counts.get(label).copied().unwrap_or((0, 0));
        if have == want {
            r.counts.insert(label.to_string(), (c.0 + 1, c.1));
            return Ok(());
        }
        r.counts.insert(label.to_string(), (c.0, c.1 + 1));
        r.changes.push(PanelChange { label: label.to_string(), number: num, section: sec, data: want.to_vec(), is_keypad: false, part });
        Ok(())
    }

    /// `DiffAsync`. `reset_missing` also compares panel records the file does not contain with factory defaults (zones, users 2+, areas 2+, keypads 2+,
    /// telephones and the report codes), so a smaller account clears what it dropped.
    pub fn diff(s: &mut ElkSession, f: &AccountFile, reset_missing: bool, send_local_code: bool) -> Result<SyncResult> {
        Self::diff_with(s, f, reset_missing, send_local_code, false)
    }

    /// [`PanelSync::diff`] that also compares the automation items (tasks, counters, thermostat names, alarm durations, custom settings, outputs, lights,
    /// voice messages, rule and text packets) when `automation` is set. Rust only; see `auto_sync`.
    pub fn diff_with(s: &mut ElkSession, f: &AccountFile, reset_missing: bool, send_local_code: bool, automation: bool) -> Result<SyncResult> {
        let mut r = SyncResult::default();
        for e in &f.users.entries {
            Self::block(s, &mut r, "user", 1, e.number, &e.bytes, 199, 0)?;
        }
        for e in &f.areas.entries {
            Self::block(s, &mut r, "area", 3, e.number, &e.bytes, 8, 0)?;
        }
        for e in &f.zones.entries {
            Self::block(s, &mut r, "zone", 5, e.number, &e.bytes, 208, 0)?;
        }
        for e in &f.keypads.entries {
            let have = s.read_keypad(e.number as u8)?;
            let c = r.counts.get("keypad").copied().unwrap_or((0, 0));
            if have.bytes[..] == e.bytes[..] {
                r.counts.insert("keypad".into(), (c.0 + 1, c.1));
                continue;
            }
            r.counts.insert("keypad".into(), (c.0, c.1 + 1));
            r.changes.push(PanelChange { label: "keypad".into(), number: e.number, section: 4, data: e.bytes.clone(), is_keypad: true, part: 0 });
        }
        for e in &f.phones.entries {
            Self::block(s, &mut r, "telephone", 8, e.number, &e.bytes, 8, 0)?;
        }
        if let Some(fw) = f.wireless()? {
            for (i, w) in fw.iter().enumerate() {
                // on the panel the part selects the block; the item number is ignored (it mirrors)
                Self::block(s, &mut r, &format!("wireless block {}", i + 1), 15, 1, w, 1, i as u8)?;
            }
        }
        if let Some(fg) = globals_48(f) {
            // Globals: send the file's settings. The local (keypad) programming code (bytes 17-22, factory default 172839) stays as the panel has it unless send_local_code is set.
            let have = s.read_block(7, 1, 0)?;
            need(&have, 48)?;
            let mut want = have.clone();
            for i in 0..48 {
                if send_local_code || !(17..=22).contains(&i) {
                    want[i] = fg[i];
                }
            }
            let c = r.counts.get("globals").copied().unwrap_or((0, 0));
            if have == want {
                r.counts.insert("globals".into(), (c.0 + 1, c.1));
            } else {
                r.counts.insert("globals".into(), (c.0, c.1 + 1));
                r.changes.push(PanelChange { label: "globals".into(), number: 1, section: 7, data: want, is_keypad: false, part: 0 });
            }
        }
        if f.has_comm() {
            for e in &f.area_rcs.entries {
                Self::block(s, &mut r, "area report codes", 9, e.number, &e.bytes, 8, 0)?;
            }
            for e in &f.user_rcs.entries {
                Self::block(s, &mut r, "user report codes", 10, e.number, &e.bytes, 199, 0)?;
            }
            Self::block(s, &mut r, "system report codes", 11, 1, &f.system_rc, 1, 0)?;
            for e in &f.zone_rcs.entries {
                Self::block(s, &mut r, "zone report codes", 12, e.number, &e.bytes, 208, 0)?;
            }
            Self::block(s, &mut r, "keypad report codes", 16, 1, &f.keypad_rc, 1, 0)?;
        }
        if reset_missing {
            fn nums(sc: &FileSection) -> HashSet<i32> {
                sc.entries.iter().map(|e| e.number).collect()
            }
            #[allow(clippy::too_many_arguments)]
            fn reset(s: &mut ElkSession, r: &mut SyncResult, label: &str, sec: u8, from: i32, to: i32, present: &HashSet<i32>, def: &dyn Fn(i32) -> Vec<u8>) -> Result<()> {
                let label = format!("{label} (reset)");
                for n in from..=to {
                    if !present.contains(&n) {
                        PanelSync::block(s, r, &label, sec, n, &def(n), to, 0)?;
                    }
                }
                Ok(())
            }
            reset(s, &mut r, "zone", 5, 1, 208, &nums(&f.zones), &DefaultRecords::zone)?;
            reset(s, &mut r, "user", 1, 2, 199, &nums(&f.users), &DefaultRecords::user)?;
            reset(s, &mut r, "area", 3, 2, 8, &nums(&f.areas), &DefaultRecords::area)?;
            reset(s, &mut r, "telephone", 8, 1, 8, &nums(&f.phones), &|_| DefaultRecords::phone())?;
            if f.has_comm() {
                reset(s, &mut r, "area report codes", 9, 2, 8, &nums(&f.area_rcs), &|_| DefaultRecords::area_rc())?;
                reset(s, &mut r, "user report codes", 10, 2, 199, &nums(&f.user_rcs), &|_| DefaultRecords::user_rc())?;
                reset(s, &mut r, "zone report codes", 12, 1, 208, &nums(&f.zone_rcs), &|_| DefaultRecords::zone_rc())?;
            }
            let have = nums(&f.keypads);
            for n in 2..=16 {
                if have.contains(&n) {
                    continue;
                }
                let k = s.read_keypad(n as u8)?;
                let c = r.counts.get("keypad (reset)").copied().unwrap_or((0, 0));
                let want = DefaultRecords::keypad();
                if k.bytes[..] == want[..] {
                    r.counts.insert("keypad (reset)".into(), (c.0 + 1, c.1));
                    continue;
                }
                r.counts.insert("keypad (reset)".into(), (c.0, c.1 + 1));
                r.changes.push(PanelChange { label: "keypad (reset)".into(), number: n, section: 4, data: want, is_keypad: true, part: 0 });
            }
        }
        if automation {
            auto_sync::diff(s, f, &mut r)?;
        }
        Ok(r)
    }

    /// `PullAsync`: reads the panel into `f`. Records already in the file are overwritten with the panel's; a record that is not in the file is added only
    /// when the panel's copy is clearly in use (a non-default name, a code, a number, a non-zero zone). Returns a note per section.
    pub fn pull(s: &mut ElkSession, f: &mut AccountFile, progress: Option<&PanelProgress>) -> Result<Vec<String>> {
        Self::pull_with(s, f, progress, false)
    }

    /// [`PanelSync::pull`] that also reads the automation items into the file when `automation` is set (see `auto_sync::pull`).
    pub fn pull_with(s: &mut ElkSession, f: &mut AccountFile, progress: Option<&PanelProgress>, automation: bool) -> Result<Vec<String>> {
        let mut notes: Vec<String> = Vec::new();
        let read = |s: &mut ElkSession, sec: u8, item: i32, part: u8| -> Result<Vec<u8>> { Ok(s.read_block(sec, item as u8, part)?) };

        if let Some(pw) = f.wireless()? {
            for i in 0..pw.len() {
                let b = read(s, 15, 1, i as u8)?;
                f.set_wireless(i, &b)?;
            }
            notes.push("wireless receiver blocks".into());
        }
        if f.globals()?.is_some() {
            let g = read(s, 7, 1, 0)?;
            f.set_globals(&g)?;
            notes.push(format!("globals: {}-digit user codes", if f.code_length() == Some(6) { "6" } else { "4" }));
        }

        let (mut upd, mut add) = (0, 0);
        for n in 1..=208 {
            let have = read(s, 5, n, 0)?;
            need(&have, 20)?;
            if f.zones.get(n).is_none() {
                let nm = name_at(&have, 4, 16)?;
                if have[0] == 0 && (nm.is_empty() || nm == format!("Zone {n:03}")) {
                    continue; // an undefined zone with a blank or default name is not brought in
                }
                f.zones.get_or_add(n)?;
                add += 1;
            } else {
                upd += 1;
            }
            if let Some(e) = f.zones.get_mut(n) {
                e.bytes = have;
            }
        }
        notes.push(format!("zones: {upd} updated, {add} added"));

        upd = 0;
        add = 0;
        for n in 1..=16 {
            let k: KeypadRecord = s.read_keypad(n as u8)?;
            if f.keypads.get(n).is_none() {
                if k.bytes[0] == 0 || name_at(&k.bytes, 6, 16)?.is_empty() {
                    continue; // no area = not in use
                }
                f.keypads.get_or_add(n)?;
                add += 1;
            } else {
                upd += 1;
            }
            if let Some(e) = f.keypads.get_mut(n) {
                e.bytes = k.bytes.to_vec();
            }
        }
        notes.push(format!("keypads: {upd} updated, {add} added"));

        // an area is brought in when the file has it, or when it is renamed or used by a zone or keypad
        let mut used: HashSet<i32> = f.keypads.entries.iter().map(|k| k.bytes[0] as i32).collect();
        for z in &f.zones.entries {
            need(&z.bytes, 3)?;
            if z.bytes[0] != 0 {
                used.insert(((z.bytes[2] >> 4) & 7) as i32 + 1);
            }
        }
        upd = 0;
        add = 0;
        for n in 1..=8 {
            let have = read(s, 3, n, 0)?;
            if f.areas.get(n).is_none() {
                if !used.contains(&n) && name_at(&have, 0, 16)? == format!("Area {n}") {
                    continue;
                }
                f.areas.get_or_add(n)?;
                add += 1;
            } else {
                upd += 1;
            }
            if let Some(e) = f.areas.get_mut(n) {
                e.bytes = have;
            }
        }
        notes.push(format!("areas: {upd} updated, {add} added"));

        upd = 0;
        add = 0;
        for n in 1..=199 {
            let exists = f.users.get(n).is_some();
            let have = read(s, 1, n, 0)?;
            if !exists {
                if have.iter().take(6).all(|&b| b == 0) {
                    continue;
                }
                f.users.get_or_add(n)?;
                add += 1;
            } else {
                upd += 1;
            }
            if let Some(e) = f.users.get_mut(n) {
                e.bytes = have;
            }
        }
        notes.push(format!("users: {upd} updated, {add} added (users with an all-zero code are not added)"));

        if f.has_phone_section() {
            upd = 0;
            add = 0;
            for n in 1..=8 {
                let have = read(s, 8, n, 0)?;
                need(&have, 22)?;
                if f.phones.get(n).is_none() {
                    if have.iter().take(20).all(|&b| b == 0) && (have[21] >> 4) == 0 {
                        continue;
                    }
                    f.phones.get_or_add(n)?;
                    add += 1;
                } else {
                    upd += 1;
                }
                if let Some(e) = f.phones.get_mut(n) {
                    e.bytes = have;
                }
            }
            notes.push(format!("telephones: {upd} updated, {add} added"));
        }

        if f.has_comm() {
            if let Some(p) = progress {
                // the report codes: one read per record now in the file, plus system and keypad
                p.add_total((f.areas.entries.len() + f.users.entries.iter().filter(|u| u.number <= 199).count() + f.zones.entries.len() + 2) as i32);
            }
            let areas: Vec<i32> = f.areas.entries.iter().map(|a| a.number).collect();
            for a in areas {
                let b = read(s, 9, a, 0)?;
                f.area_rcs.get_or_add(a)?.bytes = b;
            }
            let users: Vec<i32> = f.users.entries.iter().filter(|u| u.number <= 199).map(|u| u.number).collect();
            for u in users {
                let b = read(s, 10, u, 0)?;
                f.user_rcs.get_or_add(u)?.bytes = b;
            }
            let zones: Vec<i32> = f.zones.entries.iter().map(|z| z.number).collect();
            for z in zones {
                let b = read(s, 12, z, 0)?;
                f.zone_rcs.get_or_add(z)?.bytes = b;
            }
            let copy_into = |dst: &mut [u8], src: &[u8]| -> Result<()> {
                if src.len() > dst.len() {
                    return Err(PanelError::Elk(ElkError::Argument(
                        "Destination array was not long enough. Check the destination index, length, and the array's lower bounds. (Parameter 'destinationArray')".into(),
                    )));
                }
                dst[..src.len()].copy_from_slice(src);
                Ok(())
            };
            let sys = read(s, 11, 1, 0)?;
            copy_into(&mut f.system_rc, &sys)?;
            let kp = read(s, 16, 1, 0)?;
            copy_into(&mut f.keypad_rc, &kp)?;
            notes.push(format!(
                "report codes: {} areas, {} users, {} zones, system and keypad panic",
                f.area_rcs.entries.len(),
                f.user_rcs.entries.len(),
                f.zone_rcs.entries.len()
            ));
        }
        if automation {
            if let Some(p) = progress {
                p.add_total(auto_sync::estimate_pull(&s.panel.as_ref().map(|p| p.firmware.clone()).unwrap_or_default()));
            }
            match auto_sync::pull(s, f)? {
                Some(n) => notes.push(n),
                None => notes.push("automation items: this file has none, not read".into()),
            }
        }
        Ok(notes)
    }

    /// `ApplyAsync`: writes the changes; `progress` is called after each one.
    pub fn apply(s: &mut ElkSession, changes: &[PanelChange], mut progress: Option<&mut dyn FnMut(&PanelChange)>) -> Result<()> {
        for c in changes {
            if auto_sync::is_auto_change(c) {
                auto_sync::apply_change(s, c)?;
            } else if c.is_keypad {
                let mut k = KeypadRecord::new();
                let n = c.data.len().min(k.bytes.len());
                k.bytes[..n].copy_from_slice(&c.data[..n]);
                s.write_keypad(c.number as u8, &k)?;
            } else {
                s.write_block(c.section, c.number as u8, &c.data, c.part)?;
            }
            if let Some(p) = progress.as_mut() {
                p(c);
            }
        }
        Ok(())
    }
}
