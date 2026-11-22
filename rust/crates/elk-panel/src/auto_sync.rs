//! Panel sync of the automation items (Rust only, the C# has none): tasks, counters, thermostat names, alarm durations and custom settings,
//! outputs, lights, voice messages and the rule and text packets. The layout of every kind on the panel is in `elk_core::panel_auto`
//! (taken from ElkRP's own `Form_Receive` / `Form_Send`); the file side is `elk_account::AutomationBlock`, whose records are the same bytes
//! ElkRP keeps (`TBytes`, `CtBytes`, `CBytes`, `OPNBytes`, `X10Bytes`, `VMBytes`, `RPBytes`).
//!
//! Rules and texts share the 528 packets: rules start at packet 0, the texts are the last `T` packets, and an all-zero-start packet marks the
//! end of each. They are compared and written as two units (`rule packets`, `text packets`), the whole unit with its end marker.
//! Comments on rules live only in the editor and are never sent.

use crate::error::{PanelError, Result};
use crate::sync::{PanelChange, SyncResult};
use elk_account::{AccountFile, AutomationBlock};
use elk_core::{
    firmware_at_least, read_packet_requests, read_rules, read_texts, rules_match, texts_match, write_packet_requests, write_rules, write_texts, AutoKind, ElkError, ElkSession,
    Packet, PACKET_COUNT, PACKET_LEN, RULE_SECTION,
};

/// Label of the rule packets unit.
pub const RULES_LABEL: &str = "rule packets";
/// Label of the text packets unit.
pub const TEXTS_LABEL: &str = "text packets";

fn bad(msg: impl Into<String>) -> PanelError {
    PanelError::Elk(ElkError::InvalidData(msg.into()))
}

/// True when the change was made by [`diff`] (so [`apply_change`] handles it).
pub fn is_auto_change(c: &PanelChange) -> bool {
    c.section == RULE_SECTION || AutoKind::from_label(&c.label).is_some()
}

fn slot(a: &AutomationBlock, k: AutoKind, n: usize) -> &Vec<u8> {
    match k {
        AutoKind::Task => &a.tasks[n - 1],
        AutoKind::Counter => &a.counters[n - 1],
        AutoKind::Thermostat => &a.counters[63 + n],
        AutoKind::AlarmDuration => &a.customs[n - 1],
        AutoKind::CustomSetting => &a.customs[11 + n],
        AutoKind::Output => &a.outputs[n - 1],
        AutoKind::Light => &a.lights[n - 1],
        AutoKind::Voice => &a.voice[n - 1],
    }
}

fn slot_mut(a: &mut AutomationBlock, k: AutoKind, n: usize) -> &mut Vec<u8> {
    match k {
        AutoKind::Task => &mut a.tasks[n - 1],
        AutoKind::Counter => &mut a.counters[n - 1],
        AutoKind::Thermostat => &mut a.counters[63 + n],
        AutoKind::AlarmDuration => &mut a.customs[n - 1],
        AutoKind::CustomSetting => &mut a.customs[11 + n],
        AutoKind::Output => &mut a.outputs[n - 1],
        AutoKind::Light => &mut a.lights[n - 1],
        AutoKind::Voice => &mut a.voice[n - 1],
    }
}

/// Which items of a kind the panel firmware has: `(last item, note)`; `last == 0` means none.
fn available(k: AutoKind, firmware: &str) -> (usize, Option<String>) {
    match k {
        AutoKind::Voice if !firmware_at_least(firmware, 4, 0) => (0, Some("voice messages (firmware below 4.0)".into())),
        AutoKind::Output if !firmware_at_least(firmware, 4, 4) => (32, Some("outputs 33-64 (firmware below 4.4)".into())),
        _ => (k.count(), None),
    }
}

fn firmware_of(s: &ElkSession) -> String {
    s.panel.as_ref().map(|p| p.firmware.clone()).unwrap_or_default()
}

/// The rule packets and text packets of the block, after the checks that make them safe to write (an error describes what is wrong):
/// the counts agree with 528 and leave one packet for an end marker, no rule packet starts with two zero bytes (the panel would read it as the end).
pub fn packets_of(a: &AutomationBlock) -> Result<(Vec<Packet>, Vec<Packet>)> {
    let (r, t) = (a.rule_packets_used, a.text_packets_used);
    if r + t >= PACKET_COUNT {
        return Err(bad(format!(
            "The rules use {r} and the texts {t} of the {PACKET_COUNT} packets; at least one packet must stay free for the end marker. Nothing was compared or sent."
        )));
    }
    if a.packets.len() != PACKET_COUNT {
        return Err(bad("The automation block does not have 528 packets. Nothing was compared or sent."));
    }
    let rules: Vec<Packet> = a.packets[..r].to_vec();
    if let Some(i) = rules.iter().position(|p| p[0] == 0 && p[1] == 0) {
        return Err(bad(format!("Rule packet {} starts with two zero bytes, which the panel reads as the end of the rules. Nothing was compared or sent.", i + 1)));
    }
    let texts: Vec<Packet> = a.packets[PACKET_COUNT - t..].to_vec();
    Ok((rules, texts))
}

/// Transfers [`diff`] makes (at most; a missing voice board makes it fewer).
pub fn estimate_diff(f: &AccountFile, firmware: &str) -> i32 {
    let Some(a) = f.automation.as_ref() else {
        return 0;
    };
    let mut n = 0;
    for k in AutoKind::ALL {
        let (last, _) = available(k, firmware);
        n += last.div_ceil(k.read_batch());
    }
    n += read_packet_requests(0, a.rule_packets_used + 1) + read_packet_requests(PACKET_COUNT.saturating_sub(a.text_packets_used + 1), a.text_packets_used + 1);
    n as i32
}

/// Transfers [`pull`] makes (at most).
pub fn estimate_pull(firmware: &str) -> i32 {
    let mut n = 0;
    for k in AutoKind::ALL {
        let (last, _) = available(k, firmware);
        n += last.div_ceil(k.read_batch());
    }
    // the rule scan reads one 16-packet request per 16 rules plus the one with the end marker; the text scan the same from the top: a typical account
    // needs two requests each, counted here; a larger one makes the total grow
    (n + 4) as i32
}

/// Transfers writing these changes makes (items one each; rule and text packets 16 per request).
pub fn write_transfers(changes: &[PanelChange]) -> i32 {
    changes.iter().filter(|c| is_auto_change(c)).map(|c| if c.section == RULE_SECTION { write_packet_requests(c.data.len() / PACKET_LEN + 1) as i32 } else { 1 }).sum()
}

fn tally(r: &mut SyncResult, label: &str, same: bool) {
    let c = r.counts.get(label).copied().unwrap_or((0, 0));
    r.counts.insert(label.to_string(), if same { (c.0 + 1, c.1) } else { (c.0, c.1 + 1) });
}

fn flat(p: &[Packet]) -> Vec<u8> {
    p.iter().flatten().copied().collect()
}

/// Compares the automation items of `f` with the panel and adds what differs to `r` (counts per kind, one change per differing item, one per rule /
/// text unit, `skipped` for what this firmware does not have). Reads only; an error when the file's rule / text data cannot be written safely.
pub fn diff(s: &mut ElkSession, f: &AccountFile, r: &mut SyncResult) -> Result<()> {
    let Some(a) = f.automation.as_ref() else {
        r.skipped.push("automation items (this file has none)".into());
        return Ok(());
    };
    let (rules, texts) = packets_of(a)?; // refuse before anything is read when the file is inconsistent
    let firmware = firmware_of(s);
    for k in AutoKind::ALL {
        let (last, note) = available(k, &firmware);
        if let Some(n) = note {
            r.skipped.push(n);
        }
        if last == 0 {
            continue;
        }
        let have = match k.read_range(s, 1, last) {
            Ok(v) => v,
            Err(ElkError::InvalidData(_)) if k == AutoKind::Voice => {
                r.skipped.push("voice messages (no voice board answered)".into());
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        for (i, h) in have.iter().enumerate() {
            let n = i + 1;
            let want = slot(a, k, n);
            if want.len() != k.len() {
                return Err(bad(format!("{} {n}: the file has {} bytes, the panel item has {}. Nothing was sent.", k.label(), want.len(), k.len())));
            }
            let same = h == want;
            tally(r, k.label(), same);
            if !same {
                r.changes.push(PanelChange { label: k.label().into(), number: n as i32, section: k.locate(n).0, data: want.clone(), is_keypad: false, part: k.locate(n).1 });
            }
        }
    }
    let same = rules_match(s, &rules)?;
    tally(r, RULES_LABEL, same);
    if !same {
        r.changes.push(PanelChange { label: RULES_LABEL.into(), number: rules.len() as i32, section: RULE_SECTION, data: flat(&rules), is_keypad: false, part: 0 });
    }
    let same = texts_match(s, &texts)?;
    tally(r, TEXTS_LABEL, same);
    if !same {
        r.changes.push(PanelChange { label: TEXTS_LABEL.into(), number: texts.len() as i32, section: RULE_SECTION, data: flat(&texts), is_keypad: false, part: 1 });
    }
    Ok(())
}

fn to_packets(data: &[u8]) -> Result<Vec<Packet>> {
    if !data.len().is_multiple_of(PACKET_LEN) {
        return Err(bad("packet data is not a multiple of 8 bytes"));
    }
    Ok(data
        .chunks(PACKET_LEN)
        .map(|c| {
            let mut p = [0u8; PACKET_LEN];
            p.copy_from_slice(c);
            p
        })
        .collect())
}

/// Writes one change made by [`diff`].
pub fn apply_change(s: &mut ElkSession, c: &PanelChange) -> Result<()> {
    if c.section == RULE_SECTION {
        let p = to_packets(&c.data)?;
        if c.part == 0 {
            write_rules(s, &p)?;
        } else {
            write_texts(s, &p)?;
        }
        return Ok(());
    }
    let k = AutoKind::from_label(&c.label).ok_or_else(|| bad(format!("unknown automation change '{}'", c.label)))?;
    if c.number < 1 || c.number as usize > k.count() {
        return Err(bad(format!("{} {}: number out of range", c.label, c.number)));
    }
    Ok(k.write_one(s, c.number as usize, &c.data)?)
}

/// Reads the panel's automation items into the file (every item of the block is replaced; kinds the firmware does not have are left as they were) and
/// returns one note. Rule comments stay with a packet only while that packet is unchanged.
pub fn pull(s: &mut ElkSession, f: &mut AccountFile) -> Result<Option<String>> {
    let firmware = firmware_of(s);
    let Some(a) = f.automation.as_mut() else {
        return Ok(None);
    };
    let mut parts: Vec<String> = Vec::new();
    let mut left: Vec<String> = Vec::new();
    for k in AutoKind::ALL {
        let (last, note) = available(k, &firmware);
        if let Some(n) = note {
            left.push(n);
        }
        if last == 0 {
            continue;
        }
        let have = match k.read_range(s, 1, last) {
            Ok(v) => v,
            Err(ElkError::InvalidData(_)) if k == AutoKind::Voice => {
                left.push("voice messages (no voice board answered)".into());
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        for (i, h) in have.into_iter().enumerate() {
            *slot_mut(a, k, i + 1) = h;
        }
        parts.push(format!("{} {}", last, k.label()));
    }
    let rules = read_rules(s)?;
    let texts = read_texts(s)?;
    if rules.len() + texts.len() >= PACKET_COUNT {
        return Err(bad("The rule and text packets on the panel overlap; nothing was changed in the file."));
    }
    let old = a.packets.clone();
    let mut packets = vec![[0u8; PACKET_LEN]; PACKET_COUNT];
    packets[..rules.len()].copy_from_slice(&rules);
    packets[PACKET_COUNT - texts.len()..].copy_from_slice(&texts);
    for i in 0..PACKET_COUNT {
        if old[i] != packets[i] {
            a.comments[i].clear();
        }
    }
    a.packets = packets;
    a.rule_packets_used = rules.len();
    a.text_packets_used = texts.len();
    let mut note = format!("automation items: {}, {} rule packets, {} text packets", parts.join(", "), rules.len(), texts.len());
    if !left.is_empty() {
        note.push_str(&format!(" (left as in the file: {})", left.join(", ")));
    }
    Ok(Some(note))
}
