//! Automation items on the panel: names / tasks / counters / custom values / outputs (section 2), lighting (14), voice messages (13) and the
//! rule and text packets (121). Not in the C# `Elk.Protocol`; the oracle is ElkRP's own code (decompiled `Tasks.cs`, `Outputs.cs`,
//! `X10_Outputs.cs`, `Voice_Messages.cs`, `WhenThen.cs`, `Form_Receive` / `Form_Send`):
//!
//! | kind | section | part byte | item | bytes |
//! |---|---|---|---|---|
//! | alarm duration 1-12, custom setting 1-20 | 2 | 0 | 1-12, 13-32 | 16 |
//! | output 1-64 (more than 32 needs firmware 4.4) | 2 | 1 | 1-64 | 28 |
//! | counter 1-64, thermostat name 1-16 | 2 | 2 | 1-64, 65-80 | 12 |
//! | task 1-32 | 2 | 3 | 1-32 | 28 |
//! | lighting 1-256 | 14 | count (read 4, write 1) | 1-256 (16-bit) | 28 each |
//! | voice message 1-115 (firmware 4.0 or later) | 13 | count (read 16, write 1) | 209-323 (16-bit) | 12 each |
//! | rule / text packets | 121 (write 249) | 16 | first packet (16-bit) | 8 each, 16 per request |
//!
//! The rule packets start at packet 0 and end at the first packet whose first two bytes are 0; the text packets sit at the end (the last
//! `T` of the 528) with one such terminator packet just below them. Writes are `[section + 128, part, item_hi, item_lo, data]`; every reply
//! echoes the three bytes after the opcode (checked here, as ElkRP does).
//!
//! Only the simulated panel has been used to run the write functions here.

use crate::elk_session::ElkSession;
use crate::error::{ElkError, Result};

/// Bytes in a rule / text packet.
pub const PACKET_LEN: usize = 8;
/// Packets shared by rules and texts.
pub const PACKET_COUNT: usize = 528;
/// Packets per read / write request.
pub const PACKETS_PER_REQUEST: usize = 16;
/// Section of the rule and text packets (read opcode; the write opcode is +128 = 249).
pub const RULE_SECTION: u8 = 121;
/// Section of names, tasks, counters and custom values.
pub const NAMES_SECTION: u8 = 2;
/// Voice message section.
pub const VOICE_SECTION: u8 = 13;
/// Lighting section.
pub const LIGHT_SECTION: u8 = 14;
/// Panel item number of voice message 1 (software 4.0.0 and later).
pub const VOICE_FIRST_ITEM: u16 = 209;

/// A packet.
pub type Packet = [u8; PACKET_LEN];

/// A kind of automation item that is one fixed-size record per number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AutoKind {
    /// Alarm duration (custom value 1-12).
    AlarmDuration,
    /// Custom setting (custom value 13-32).
    CustomSetting,
    /// Output name and settings.
    Output,
    /// Counter.
    Counter,
    /// Thermostat name.
    Thermostat,
    /// Task.
    Task,
    /// Lighting device.
    Light,
    /// Voice message.
    Voice,
}

impl AutoKind {
    /// All kinds, in the order they are compared.
    pub const ALL: [AutoKind; 8] =
        [AutoKind::Task, AutoKind::Counter, AutoKind::Thermostat, AutoKind::AlarmDuration, AutoKind::CustomSetting, AutoKind::Output, AutoKind::Light, AutoKind::Voice];

    /// The label used in compare results (singular, like the other sections).
    pub fn label(self) -> &'static str {
        match self {
            AutoKind::AlarmDuration => "alarm duration",
            AutoKind::CustomSetting => "custom setting",
            AutoKind::Output => "output",
            AutoKind::Counter => "counter",
            AutoKind::Thermostat => "thermostat",
            AutoKind::Task => "task",
            AutoKind::Light => "light",
            AutoKind::Voice => "voice message",
        }
    }

    /// The kind with this label.
    pub fn from_label(l: &str) -> Option<AutoKind> {
        AutoKind::ALL.into_iter().find(|k| k.label() == l)
    }

    /// How many numbered items there are (numbers 1..=count).
    pub fn count(self) -> usize {
        match self {
            AutoKind::AlarmDuration => 12,
            AutoKind::CustomSetting => 20,
            AutoKind::Output => 64,
            AutoKind::Counter => 64,
            AutoKind::Thermostat => 16,
            AutoKind::Task => 32,
            AutoKind::Light => 256,
            AutoKind::Voice => 115,
        }
    }

    /// Bytes per item.
    pub fn len(self) -> usize {
        match self {
            AutoKind::AlarmDuration | AutoKind::CustomSetting => 16,
            AutoKind::Counter | AutoKind::Thermostat | AutoKind::Voice => 12,
            AutoKind::Output | AutoKind::Task | AutoKind::Light => 28,
        }
    }

    /// Never empty (clippy).
    pub fn is_empty(self) -> bool {
        false
    }

    /// Items per read request.
    pub fn read_batch(self) -> usize {
        match self {
            AutoKind::Light => 4,
            AutoKind::Voice => 16,
            _ => 1,
        }
    }

    /// `(section, part byte, panel item)` of item `n` (1-based) when it is read or written on its own.
    pub fn locate(self, n: usize) -> (u8, u8, u16) {
        let n = n as u16;
        match self {
            AutoKind::AlarmDuration => (NAMES_SECTION, 0, n),
            AutoKind::CustomSetting => (NAMES_SECTION, 0, n + 12),
            AutoKind::Output => (NAMES_SECTION, 1, n),
            AutoKind::Counter => (NAMES_SECTION, 2, n),
            AutoKind::Thermostat => (NAMES_SECTION, 2, n + 64),
            AutoKind::Task => (NAMES_SECTION, 3, n),
            AutoKind::Light => (LIGHT_SECTION, 1, n),
            AutoKind::Voice => (VOICE_SECTION, 1, n + VOICE_FIRST_ITEM - 1),
        }
    }

    /// The first software version (major, minor) of the panel that has item `n` (outputs above 32: 4.4; voice messages: 4.0), or None.
    pub fn needs_firmware(self, n: usize) -> Option<(u32, u32)> {
        match self {
            AutoKind::Output if n > 32 => Some((4, 4)),
            AutoKind::Voice => Some((4, 0)),
            _ => None,
        }
    }

    /// Reads items `from..=to` (1-based), batched for lights and voice messages. A reply that is too short is an `InvalidData` error (for
    /// voice messages ElkRP reads that as "the plug-in voice board is missing").
    pub fn read_range(self, s: &mut ElkSession, from: usize, to: usize) -> Result<Vec<Vec<u8>>> {
        let mut out = Vec::new();
        let mut n = from;
        while n <= to {
            let k = self.read_batch().min(to - n + 1);
            let (sec, part, item) = self.locate(n);
            let part = if self.read_batch() > 1 { k as u8 } else { part };
            let data = s.read_block16(sec, part, item, k * self.len())?;
            for i in 0..k {
                out.push(data[i * self.len()..(i + 1) * self.len()].to_vec());
            }
            n += k;
        }
        Ok(out)
    }

    /// Writes item `n` (one item per request, which ElkRP does for a single item too).
    pub fn write_one(self, s: &mut ElkSession, n: usize, data: &[u8]) -> Result<()> {
        if data.len() != self.len() {
            return Err(ElkError::Argument(format!("{} {n}: {} bytes, expected {}", self.label(), data.len(), self.len())));
        }
        let (sec, part, item) = self.locate(n);
        let part = if self.read_batch() > 1 { 1 } else { part };
        s.write_block16(sec, part, item, data)
    }
}

/// True when a panel firmware text (`5.2.9`) is at least `major.minor`.
pub fn firmware_at_least(firmware: &str, major: u32, minor: u32) -> bool {
    let mut it = firmware.split('.').map(|p| p.trim().parse::<u32>().unwrap_or(0));
    let (a, b) = (it.next().unwrap_or(0), it.next().unwrap_or(0));
    (a, b) >= (major, minor)
}

fn is_terminator(p: &Packet) -> bool {
    p[0] == 0 && p[1] == 0
}

fn flat(packets: &[Packet]) -> Vec<u8> {
    packets.iter().flatten().copied().collect()
}

/// Reads packets `first..first + count` (whole 16-packet requests on 16-packet boundaries, like ElkRP).
pub fn read_packets(s: &mut ElkSession, first: usize, count: usize) -> Result<Vec<Packet>> {
    if first + count > PACKET_COUNT {
        return Err(ElkError::OutOfRange("packet range outside the 528 rule and text packets".into()));
    }
    let mut out: Vec<Packet> = Vec::with_capacity(count);
    let mut chunk = first / PACKETS_PER_REQUEST * PACKETS_PER_REQUEST;
    while chunk < first + count {
        let data = s.read_block16(RULE_SECTION, PACKETS_PER_REQUEST as u8, chunk as u16, PACKETS_PER_REQUEST * PACKET_LEN)?;
        for i in 0..PACKETS_PER_REQUEST {
            let idx = chunk + i;
            if idx >= first && idx < first + count {
                let mut p = [0u8; PACKET_LEN];
                p.copy_from_slice(&data[i * PACKET_LEN..(i + 1) * PACKET_LEN]);
                out.push(p);
            }
        }
        chunk += PACKETS_PER_REQUEST;
    }
    Ok(out)
}

/// Writes `packets` starting at packet `first`, 16 per request.
pub fn write_packets(s: &mut ElkSession, first: usize, packets: &[Packet]) -> Result<()> {
    if first + packets.len() > PACKET_COUNT {
        return Err(ElkError::OutOfRange("packet range outside the 528 rule and text packets".into()));
    }
    let mut at = 0;
    while at < packets.len() {
        let k = PACKETS_PER_REQUEST.min(packets.len() - at);
        s.write_block16(RULE_SECTION, k as u8, (first + at) as u16, &flat(&packets[at..at + k]))?;
        at += k;
    }
    Ok(())
}

/// Reads the rule packets the way ElkRP's `Form_Receive("Rules")` does: from packet 0 up to (not including) the first packet that starts with two
/// zero bytes. An error when there is no such packet in the 528.
pub fn read_rules(s: &mut ElkSession) -> Result<Vec<Packet>> {
    let mut out = Vec::new();
    let mut chunk = 0;
    while chunk < PACKET_COUNT {
        for p in read_packets(s, chunk, PACKETS_PER_REQUEST)? {
            if is_terminator(&p) {
                return Ok(out);
            }
            out.push(p);
        }
        chunk += PACKETS_PER_REQUEST;
    }
    Err(ElkError::InvalidData("The rule packets on the panel have no end marker.".into()))
}

/// Reads the text packets the way ElkRP's `Form_Receive("Texts")` does: from the last packet downwards to the first packet that starts with two
/// zero bytes. Returned in ascending (file) order. (A text of exactly 8 characters ends in an all-zero packet that this scan cannot tell from the
/// end marker; compare uses [`texts_match`] instead, which does not have that blind spot.)
pub fn read_texts(s: &mut ElkSession) -> Result<Vec<Packet>> {
    let mut down: Vec<Packet> = Vec::new();
    let mut chunk = PACKET_COUNT - PACKETS_PER_REQUEST;
    loop {
        let ps = read_packets(s, chunk, PACKETS_PER_REQUEST)?;
        for p in ps.iter().rev() {
            if is_terminator(p) {
                down.reverse();
                return Ok(down);
            }
            down.push(*p);
        }
        if chunk == 0 {
            return Err(ElkError::InvalidData("The text packets on the panel have no end marker.".into()));
        }
        chunk -= PACKETS_PER_REQUEST;
    }
}

/// True when the panel's rule packets are exactly `packets` followed by an end marker (reads `packets.len() + 1` packets).
pub fn rules_match(s: &mut ElkSession, packets: &[Packet]) -> Result<bool> {
    let have = read_packets(s, 0, packets.len() + 1)?;
    Ok(have[..packets.len()] == *packets && is_terminator(&have[packets.len()]))
}

/// True when the panel's last `packets.len()` packets are exactly `packets` and the packet below them is an end marker.
pub fn texts_match(s: &mut ElkSession, packets: &[Packet]) -> Result<bool> {
    let end = PACKET_COUNT - packets.len();
    if end == 0 {
        return Err(ElkError::InvalidData("The text packets fill all 528 packets; there is no room for the end marker.".into()));
    }
    let have = read_packets(s, end - 1, packets.len() + 1)?;
    Ok(is_terminator(&have[0]) && have[1..] == *packets)
}

/// Writes the rule packets and the end marker after them. Refuses when there is no room for the marker.
pub fn write_rules(s: &mut ElkSession, packets: &[Packet]) -> Result<()> {
    if packets.len() >= PACKET_COUNT {
        return Err(ElkError::OutOfRange("the rules need all 528 packets; there is no room for the end marker".into()));
    }
    let mut all = packets.to_vec();
    all.push([0; PACKET_LEN]);
    write_packets(s, 0, &all)
}

/// Writes the end marker and then the text packets at the end of the 528. Refuses when there is no room for the marker.
pub fn write_texts(s: &mut ElkSession, packets: &[Packet]) -> Result<()> {
    if packets.len() >= PACKET_COUNT {
        return Err(ElkError::OutOfRange("the texts need all 528 packets; there is no room for the end marker".into()));
    }
    let mut all = vec![[0u8; PACKET_LEN]];
    all.extend_from_slice(packets);
    write_packets(s, PACKET_COUNT - packets.len() - 1, &all)
}

/// Number of requests [`read_packets`] makes for this range.
pub fn read_packet_requests(first: usize, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    (first + count).div_ceil(PACKETS_PER_REQUEST) - first / PACKETS_PER_REQUEST
}

/// Number of requests [`write_packets`] makes for `count` packets.
pub fn write_packet_requests(count: usize) -> usize {
    count.div_ceil(PACKETS_PER_REQUEST)
}

/// Number of requests a full scan makes at most (used for the backup estimate): the rules and texts of a typical account are small, so a backup
/// counts the 33 requests that cover all 528 packets.
pub const PACKET_REQUESTS_ALL: usize = PACKET_COUNT / PACKETS_PER_REQUEST;

/// Transfers a read of every item of every kind makes (without rules and texts).
pub fn estimate_item_reads() -> i32 {
    AutoKind::ALL.iter().map(|k| k.count().div_ceil(k.read_batch()) as i32).sum()
}

/// Reads all 528 rule and text packets exactly as the panel holds them (33 requests); used by the backup, which therefore does not depend on the
/// end-marker scan.
pub fn read_all_packets(s: &mut ElkSession) -> Result<Vec<Packet>> {
    read_packets(s, 0, PACKET_COUNT)
}

/// Writes all 528 packets back (33 requests).
pub fn write_all_packets(s: &mut ElkSession, packets: &[Packet]) -> Result<()> {
    if packets.len() != PACKET_COUNT {
        return Err(ElkError::Argument(format!("{} packets, expected {PACKET_COUNT}", packets.len())));
    }
    write_packets(s, 0, packets)
}

/// Reads, for a backup, every automation item of every kind the firmware has (outputs above 32 and voice messages need newer firmware; a voice
/// board that is not fitted is left out). Returns `(kind, number, data)`.
pub fn read_all_items(s: &mut ElkSession, firmware: &str) -> Result<Vec<(AutoKind, usize, Vec<u8>)>> {
    let mut out = Vec::new();
    for k in AutoKind::ALL {
        let last = match k {
            AutoKind::Output if !firmware_at_least(firmware, 4, 4) => 32,
            AutoKind::Voice if !firmware_at_least(firmware, 4, 0) => continue,
            _ => k.count(),
        };
        match k.read_range(s, 1, last) {
            Ok(v) => {
                for (i, d) in v.into_iter().enumerate() {
                    out.push((k, i + 1, d));
                }
            }
            Err(ElkError::InvalidData(_)) if k == AutoKind::Voice => {} // no voice board
            Err(e) => return Err(e),
        }
    }
    Ok(out)
}

/// Transfers [`read_all_items`] makes at most plus the 33 packet requests.
pub fn estimate_backup_transfers() -> i32 {
    estimate_item_reads() + PACKET_REQUESTS_ALL as i32
}
