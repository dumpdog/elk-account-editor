//! Port of PanelClockLog.cs: the panel clock, the RP access code change and the event log.

use crate::elk_session::ElkSession;
use crate::error::{ElkError, Result};
use crate::framing::FrameKind;
use chrono::{Datelike, NaiveDate, NaiveDateTime, Timelike};

fn bcd(v: i32) -> u8 {
    (((v / 10) << 4) | (v % 10)) as u8
}

fn from_bcd(b: u8) -> i32 {
    ((b >> 4) as i32) * 10 + (b & 15) as i32
}

/// The panel's clock, from ElkRP's "Set Time and Date" window. Read: `7F 00 00 03`; the reply holds seconds, minutes, hours (BCD) at
/// [4..6], the day of week (Sunday = 1) at [7], then day, month and year minus 2000 (BCD) at [8..10]. Set: `FF 00 00 03` followed by the
/// same seven bytes; the panel answers with an ACK.
pub struct PanelClock;

impl PanelClock {
    /// The set-clock request for `t`.
    pub fn set_request(t: NaiveDateTime) -> Vec<u8> {
        vec![
            0xFF,
            0,
            0,
            3,
            bcd(t.second() as i32),
            bcd(t.minute() as i32),
            bcd(t.hour() as i32),
            (t.weekday().num_days_from_sunday() + 1) as u8,
            bcd(t.day() as i32),
            bcd(t.month() as i32),
            bcd(t.year() - 2000),
        ]
    }

    /// Decodes a read reply; `None` if the panel's clock is not valid (day or month zero is treated as 1, as ElkRP does).
    pub fn decode(r: &[u8]) -> Option<NaiveDateTime> {
        if r.len() < 11 {
            return None;
        }
        let day = from_bcd(r[8]);
        let month = from_bcd(r[9]);
        NaiveDate::from_ymd_opt(from_bcd(r[10]) + 2000, if month == 0 { 1 } else { month } as u32, if day == 0 { 1 } else { day } as u32)?.and_hms_opt(
            from_bcd(r[6]) as u32,
            from_bcd(r[5]) as u32,
            from_bcd(r[4]) as u32,
        )
    }

    /// Reads the clock.
    pub fn read(s: &mut ElkSession) -> Result<Option<NaiveDateTime>> {
        s.send(&[0x7F, 0, 0, 3])?;
        let r = s.receive(0x7F)?;
        if r.len() >= 11 && r[3] == 3 {
            Ok(Self::decode(&r))
        } else {
            Err(ElkError::InvalidData("unexpected reply to the clock query".into()))
        }
    }

    /// Sets the clock and reports whether the panel acknowledged. Day of week follows the date given.
    pub fn set(s: &mut ElkSession, t: NaiveDateTime) -> Result<bool> {
        if !(2000..=2099).contains(&t.year()) {
            return Err(ElkError::OutOfRange("the panel keeps years 2000 to 2099".into()));
        }
        s.send(&Self::set_request(t))?;
        match s.receive_any_frame() {
            Ok(f) => Ok(f.kind == FrameKind::Ack),
            Err(e) if e.is_timeout() => Ok(false),
            Err(e) => Err(e),
        }
    }
}

/// Changes the RP access code (the code ElkRP and this editor log in with), as the Change button in ElkRP's account details does:
/// `7F 00 00 05` followed by the six digits, last digit first (the same order as the login), answered with an ACK.
pub struct PanelAccess;

impl PanelAccess {
    /// Exactly six ASCII digits.
    pub fn is_valid(code: &str) -> bool {
        code.encode_utf16().count() == 6 && code.chars().all(|c| c.is_ascii_digit())
    }

    /// The change request.
    pub fn change_request(new_code: &str) -> Result<Vec<u8>> {
        if !Self::is_valid(new_code) {
            return Err(ElkError::Argument("The RP access code is exactly six digits.".into()));
        }
        let c = new_code.as_bytes();
        let mut p = vec![0u8; 10];
        p[0] = 0x7F;
        p[3] = 5;
        for n in 1..=6 {
            p[10 - n] = c[n - 1] - b'0'; // digit 1 goes last, digit 6 first
        }
        Ok(p)
    }

    /// Sends the change and reports whether the panel acknowledged it.
    pub fn change(s: &mut ElkSession, new_code: &str) -> Result<bool> {
        s.send(&Self::change_request(new_code)?)?;
        match s.receive_any_frame() {
            Ok(f) => Ok(f.kind == FrameKind::Ack),
            Err(e) if e.is_timeout() => Ok(false),
            Err(e) => Err(e),
        }
    }
}

/// One entry of the panel's event log. `event` is the panel's event number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    /// 1-based position, newest first.
    pub index: i32,
    /// Time stamp (`None` for the end marker or an invalid date).
    pub time: Option<NaiveDateTime>,
    /// Area 1-8.
    pub area: i32,
    /// Event number.
    pub event: i32,
    /// Item number.
    pub item: i32,
    /// Event 4095.
    pub end_of_log: bool,
}

/// The event log, from ElkRP's "Receive Log" window. Request `7B 14 hi lo` (20 entries, starting at address 65535 for the newest); the reply
/// holds the number of entries returned at [1], the address of the next block at [2..3] and 8 bytes per entry from [4]. An entry packs, from
/// the first byte: year-2000 (5 bits), month (4), day (5), hour (5), minute (6), day of week (3), area-1 (3), event number (12),
/// item number-1 (9) and a 3-bit tail. Event 0 or an all-zero date ends the list; event 4095 is the "END OF LOG" marker, which the real panel
/// returns in the middle of the list (older events follow), so it is skipped. The log holds 512 entries.
pub struct PanelLog;

impl PanelLog {
    /// Number of entries the panel keeps.
    pub const CAPACITY: i32 = 512;

    /// Decodes one 8-byte entry at `offset` (needs 7 bytes from there).
    pub fn decode_entry(e: &[u8], offset: usize, index: i32) -> Option<LogEntry> {
        let (b0, b1, b2, b3, b4, b5, b6) =
            (e[offset] as i32, e[offset + 1] as i32, e[offset + 2] as i32, e[offset + 3] as i32, e[offset + 4] as i32, e[offset + 5] as i32, e[offset + 6] as i32);
        let year = (b0 >> 3) & 0x1F;
        let month = ((b0 & 7) << 1) | (b1 >> 7);
        let day = (b1 >> 2) & 0x1F;
        let hour = ((b1 & 3) << 3) | (b2 >> 5);
        let minute = ((b2 & 0x1F) << 1) | (b3 >> 7);
        let area = ((b3 >> 1) & 7) + 1;
        let ev = b4 * 16 + (b5 >> 4);
        let item = (((b5 & 0xF) << 5) | ((b6 >> 3) & 0x1F)) + 1;
        if ev == 0 || (year == 0 && month == 0 && day == 0) {
            return None;
        }
        if ev == 4095 {
            return Some(LogEntry { index, time: None, area, event: ev, item, end_of_log: true });
        }
        let time = NaiveDate::from_ymd_opt(2000 + year, month as u32, day as u32).and_then(|d| d.and_hms_opt(hour as u32, minute as u32, 0));
        Some(LogEntry { index, time, area, event: ev, item, end_of_log: false })
    }

    /// Decodes a log reply into its entries (stops at the first empty one). Returns the entries, the address of the next block and
    /// whether the list ended. `max_slots` limits how many slots are looked at (`i32::MAX` for no limit).
    pub fn decode_reply(r: &[u8], first_index: i32, max_slots: i32) -> (Vec<LogEntry>, i32, bool) {
        let mut list: Vec<LogEntry> = Vec::new();
        let mut ended = false;
        let count = (if r.len() > 1 { r[1] as i32 } else { 0 }).min(max_slots);
        let next = if r.len() > 3 { r[2] as i32 * 256 + r[3] as i32 } else { 0 };
        let mut i = 0i32;
        while i < count && (4 + 8 * i as usize + 7) < r.len() {
            match Self::decode_entry(r, 4 + 8 * i as usize, first_index + list.len() as i32) {
                None => {
                    ended = true; // an empty slot: nothing older
                    break;
                }
                Some(e) if e.end_of_log => {} // event 4095 marks the write position, it is not an event
                Some(e) => list.push(e),
            }
            i += 1;
        }
        (list, next, ended || count == 0)
    }

    /// Reads up to `max` entries, newest first. Read only.
    pub fn read(s: &mut ElkSession, max: i32) -> Result<Vec<LogEntry>> {
        // ElkRP first sends 7B 00 00 00. On the real panel that request writes an END OF LOG marker into the log; it is a write, so it is
        // not sent here. Reading from address 65535 returns the newest entries first without it.
        let mut all: Vec<LogEntry> = Vec::new();
        let mut next = 65535i32;
        let mut slots = 0i32;
        let limit = max.min(Self::CAPACITY);
        while (all.len() as i32) < limit && slots < Self::CAPACITY {
            s.send(&[0x7B, 20, (next / 256) as u8, (next & 0xFF) as u8])?;
            let r = s.receive(0x7B)?;
            let (entries, n, ended) = Self::decode_reply(&r, all.len() as i32 + 1, Self::CAPACITY - slots); // the log is a ring of 512 slots: one lap
            let room = (limit - all.len() as i32).max(0) as usize;
            all.extend(entries.into_iter().take(room));
            slots += if r.len() > 1 { r[1] as i32 } else { 0 };
            if ended || n == next {
                break;
            }
            next = n;
        }
        Ok(all)
    }
}
