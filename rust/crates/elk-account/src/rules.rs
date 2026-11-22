//! Port of `Elk.Protocol/Rules.cs`: the rule packets of an [`AutomationBlock`] as ElkRP "When / Then" rules (structural view, not
//! the English grammar). Packet type = byte 0 / 8; the result half of a packet has its own type in byte 4 / 8.

#![allow(clippy::needless_range_loop)]

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::automation::{
    text_strings_in, AutomationBlock, AutomationMeta, OrderedMap, EVENT_TSV, PACKET_COUNT,
    PACKET_LEN,
};
use crate::error::{parse_int_loose, Error, Result};

/// One automation rule: a group of 8-byte packets, an optional comment and a one-line reading.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RuleDto {
    pub n: i32,
    pub comment: String,
    pub packets: Vec<Vec<i32>>,
    pub summary: String,
    /// Editor-side flag, ElkRP's per-rule `Disabled` (kept only in its database, so the export file cannot hold it): a disabled rule
    /// is skipped by a send to the panel. Not written to the file; omitted from the JSON when false.
    #[serde(skip_serializing_if = "is_false")]
    pub disabled: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn ptype(p: &[u8]) -> u8 {
    p[0] / 8
}

fn rtype(p: &[u8]) -> u8 {
    p[4] / 8
}

/// True when this build has the event table (the public source tree does not include it).
pub fn event_table_present() -> bool {
    !EVENT_TSV.is_empty()
}

/// Event names from ElkRP's event table (EventLists, M1G 5.3.0): event number to text, e.g. 1003 = BURGLAR ALARM, ANY AREA.
pub fn event_names() -> &'static OrderedMap {
    static M: OnceLock<OrderedMap> = OnceLock::new();
    M.get_or_init(|| {
        let mut d = OrderedMap::default();
        for line in AutomationMeta::load_lines(EVENT_TSV) {
            let t: Vec<&str> = line.trim_end_matches('\r').split('\t').collect();
            if t.len() < 2 {
                continue;
            }
            // "1003 = BURGLAR ALARM, ANY AREA"
            if let Some(eq) = t[1].find(" = ") {
                if eq > 0 {
                    if let Some(n) = parse_int_loose(&t[1][..eq]) {
                        d.insert(n, t[1][eq + 3..].to_string());
                    }
                }
            }
        }
        d
    })
}

/// Splits the used packets into rules: (start, count) ranges. Stops at the first packet with byte 0 == 0 or at the used count.
pub fn group(a: &AutomationBlock) -> Vec<(usize, usize)> {
    let mut res = Vec::new();
    let limit = a.rule_packets_used.min(PACKET_COUNT);
    let pk = |i: usize| &a.packets[i];
    let mut p = 0;
    while p < limit && pk(p)[0] != 0 {
        let start = p;
        if matches!(ptype(pk(p)), 18 | 19) {
            // stray continuation packet: keep it as a rule of its own so nothing is lost
            p += 1;
            res.push((start, 1));
            continue;
        }
        while p + 1 < limit && rtype(pk(p)) == 1 && !matches!(ptype(pk(p + 1)), 18 | 19) {
            p += 1;
        }
        if rtype(pk(p)) > 1 {
            p += if p + 1 < limit && ptype(pk(p + 1)) == 19 {
                2
            } else {
                1
            };
        } else {
            p += 1;
        }
        while p < limit && ptype(pk(p)) == 18 {
            p += if p + 1 < limit && ptype(pk(p + 1)) == 19 {
                2
            } else {
                1
            };
        }
        res.push((start, p - start));
    }
    // anything left in the used area that is not covered is kept as one trailing group
    if p < limit {
        res.push((p, limit - p));
    }
    res
}

pub fn decode(a: &AutomationBlock) -> Vec<RuleDto> {
    let mut rules = Vec::new();
    for (n, (start, count)) in group(a).into_iter().enumerate() {
        let mut r = RuleDto {
            n: n as i32 + 1,
            comment: a.comments[n].clone(),
            packets: (0..count)
                .map(|i| a.packets[start + i].iter().map(|&b| b as i32).collect())
                .collect(),
            summary: String::new(),
            disabled: false,
        };
        r.summary = summarise(a, &r);
        rules.push(r);
    }
    rules
}

fn bytes_of(p: &[i32]) -> Vec<u8> {
    p.iter().map(|&v| v as u8).collect()
}

fn summarise(a: &AutomationBlock, r: &RuleDto) -> String {
    let first = bytes_of(&r.packets[0]);
    if matches!(ptype(&first), 18 | 19) {
        return "continuation packet".to_string();
    }
    let mut sb = String::from("WHENEVER ");
    sb.push_str(&trigger(a, &first));
    for (i, raw) in r.packets.iter().enumerate() {
        let p = bytes_of(raw);
        if i > 0 && !matches!(ptype(&p), 18 | 19) {
            sb.push_str(" AND ");
            sb.push_str(&trigger(a, &p));
        }
        if ptype(&p) == 19 {
            sb.push_str(" FOR ");
            sb.push_str(&for_time(&p));
            continue;
        }
        if rtype(&p) > 1 {
            sb.push_str(" THEN ");
            sb.push_str(&result(a, &p));
        }
    }
    sb
}

fn trigger(a: &AutomationBlock, p: &[u8]) -> String {
    let t = ptype(p);
    let ev = p[2] as i32 * 256 + p[3] as i32;
    if t == 11 {
        if let Some(name) = event_names().get(ev) {
            return if p[1] == 0 {
                name.to_string()
            } else {
                format!("{name} (item {})", p[1])
            };
        }
        return if event_table_present() {
            format!("event {ev}")
        } else {
            format!("Event {ev}")
        };
    }
    if t == 13 {
        return format!("text received \"{}\"", text_at(a, ev as usize));
    }
    format!("trigger type {t} ({},{},{},{})", p[0] & 7, p[1], p[2], p[3])
}

fn result(a: &AutomationBlock, p: &[u8]) -> String {
    let t = rtype(p);
    if t == 7 || t == 8 {
        let ptr = (p[6] & 7) as usize * 256 + p[7] as usize;
        return format!("result type {t}: text \"{}\"", text_at(a, ptr));
    }
    format!("result type {t} ({},{},{},{})", p[4] & 7, p[5], p[6], p[7])
}

fn for_time(p: &[u8]) -> String {
    // int arithmetic as in C# (unchecked, wraps)
    let secs: i32 = if p[1] > 0 {
        -(p[1] as i32)
    } else {
        (((p[4] as i32) * 256 + p[5] as i32).wrapping_mul(256) + p[6] as i32)
            .wrapping_mul(256)
            .wrapping_add(p[7] as i32)
    };
    if secs < 0 {
        let m = secs.wrapping_neg();
        format!("{m} (counter/timer {m})")
    } else {
        format!("{secs} s")
    }
}

fn text_at(a: &AutomationBlock, packet: usize) -> String {
    let first = PACKET_COUNT - a.text_packets_used;
    if packet < first || packet >= PACKET_COUNT {
        return "?".to_string();
    }
    text_strings_in(&a.packets, PACKET_COUNT - packet)
        .into_iter()
        .next()
        .unwrap_or_default()
}

// ---- text pointers ----------------------------------------------------------------------------------------------------

/// Where a packet points at a text string (as a packet number): a trigger of type 13 (bytes 2-3) and a THEN of type 7 or 8
/// (low 3 bits of byte 6, and byte 7). (packet index, pointer, is the result pointer)
fn pointers(a: &AutomationBlock) -> Vec<(usize, usize, bool)> {
    let mut v = Vec::new();
    for i in 0..a.rule_packets_used.min(PACKET_COUNT) {
        let p = &a.packets[i];
        if ptype(p) == 13 {
            v.push((i, p[2] as usize * 256 + p[3] as usize, false));
        }
        if matches!(rtype(p), 7 | 8) {
            v.push((i, (p[6] & 7) as usize * 256 + p[7] as usize, true));
        }
    }
    v
}

/// The packet numbers of the text strings that rules refer to.
pub fn text_refs(a: &AutomationBlock) -> HashSet<usize> {
    pointers(a).into_iter().map(|x| x.1).collect()
}

/// Re-points rules after the text strings moved. `map` is old start packet to new start packet.
pub fn remap_text_refs(a: &mut AutomationBlock, map: &HashMap<usize, usize>) {
    for (i, ptr, result) in pointers(a) {
        let Some(&to) = map.get(&ptr) else { continue };
        if to == ptr {
            continue;
        }
        let p = &mut a.packets[i];
        if result {
            p[6] = (p[6] & 0xF8) | (to >> 8) as u8;
            p[7] = to as u8;
        } else {
            p[2] = (to >> 8) as u8;
            p[3] = to as u8;
        }
    }
}

/// 0-based indexes (into the rule list) of the rules the editor marks disabled.
pub fn disabled_indexes(rules: &[RuleDto]) -> HashSet<usize> {
    rules
        .iter()
        .enumerate()
        .filter(|(_, r)| r.disabled)
        .map(|(i, _)| i)
        .collect()
}

/// What ElkRP's send does with disabled rules: their packets are left out and the enabled ones follow each other from packet 0
/// (the end marker is the zero packet after the last enabled rule). `disabled` holds 0-based rule indexes (the groups of [`group`]);
/// the comments move with their rules. Returns how many rules were removed. The text packets (the top of the area) are untouched.
pub fn drop_disabled(a: &mut AutomationBlock, disabled: &HashSet<usize>) -> usize {
    let groups = group(a);
    let removed = groups
        .iter()
        .enumerate()
        .filter(|(i, _)| disabled.contains(i))
        .count();
    if removed == 0 {
        return 0;
    }
    let old = a.packets.clone();
    let old_comments = a.comments.clone();
    let free = PACKET_COUNT - a.text_packets_used;
    for p in a.packets.iter_mut().take(free) {
        *p = [0; PACKET_LEN];
    }
    for c in a.comments.iter_mut() {
        c.clear();
    }
    let (mut at, mut n) = (0, 0);
    for (i, &(s, c)) in groups.iter().enumerate() {
        if disabled.contains(&i) {
            continue;
        }
        a.packets[at..at + c].copy_from_slice(&old[s..s + c]);
        a.comments[n] = old_comments[i].clone();
        at += c;
        n += 1;
    }
    a.rule_packets_used = at;
    removed
}

// ---- apply --------------------------------------------------------------------------------------------------------------

/// Writes the rule list back. The rule packets are rebuilt only when they (or their order or count) differ from the file;
/// comments are written per rule.
pub fn apply(rules: &[RuleDto], a: &mut AutomationBlock, text_packets: usize) -> Result<()> {
    for r in rules {
        if r.packets.is_empty() {
            return Err(Error::format(format!(
                "rule {}: a rule needs at least one packet",
                r.n
            )));
        }
        for p in &r.packets {
            if p.len() != 8 || p.iter().any(|v| !(0..=255).contains(v)) {
                return Err(Error::format(format!(
                    "rule {}: a packet is 8 values of 0-255",
                    r.n
                )));
            }
        }
        if r.packets[0][0] == 0 {
            return Err(Error::format(format!(
                "rule {}: the first packet cannot start with 0 (that ends the rule list)",
                r.n
            )));
        }
        if r.comment.chars().any(|c| (c as u32) < 32) || r.comment.encode_utf16().count() > 250 {
            return Err(Error::format(format!(
                "rule {}: the comment must be one line of printable characters",
                r.n
            )));
        }
    }
    let current = group(a);
    // rules that are not in the file as they are get checked (an old rule of an odd kind never blocks a save)
    let existing: HashSet<Vec<Vec<u8>>> = current
        .iter()
        .map(|&(s, c)| (0..c).map(|k| a.packets[s + k].to_vec()).collect())
        .collect();
    for r in rules {
        let key: Vec<Vec<u8>> = r.packets.iter().map(|p| bytes_of(p)).collect();
        if !existing.contains(&key) {
            validate_rule(r, text_packets)?;
        }
    }
    let mut same = current.len() == rules.len();
    for (i, &(s, c)) in current.iter().enumerate() {
        if !same {
            break;
        }
        same = rules[i].packets.len() == c;
        for k in 0..c {
            if !same {
                break;
            }
            same = bytes_of(&rules[i].packets[k]) == a.packets[s + k];
        }
    }
    if !same {
        let total: usize = rules.iter().map(|r| r.packets.len()).sum();
        let free = PACKET_COUNT - text_packets;
        if total > free {
            return Err(Error::format(format!(
                "the rules need {total} packets but only {free} are free (text strings use {text_packets} of {PACKET_COUNT})"
            )));
        }
        for p in a.packets.iter_mut().take(free) {
            *p = [0; PACKET_LEN];
        }
        let mut at = 0;
        for r in rules {
            for p in &r.packets {
                for k in 0..8 {
                    a.packets[at][k] = p[k] as u8;
                }
                at += 1;
            }
        }
        a.rule_packets_used = total;
    }
    for (i, r) in rules.iter().enumerate() {
        if a.comments[i] != r.comment {
            a.comments[i] = r.comment.clone();
        }
    }
    if !same {
        for i in rules.len()..PACKET_COUNT {
            if !a.comments[i].is_empty() {
                a.comments[i].clear();
            }
        }
    }
    Ok(())
}

// ---- validation of a new or changed rule ------------------------------------------------------------------------------

const TRIGGER_TYPES: [u8; 13] = [2, 3, 4, 9, 10, 11, 12, 13, 14, 16, 20, 21, 23];
const CONDITION_TYPES: [u8; 15] = [2, 5, 6, 7, 8, 9, 10, 11, 12, 14, 16, 17, 21, 22, 23];
const RESULT_TYPES: [u8; 26] = [
    2, 3, 4, 5, 7, 8, 9, 10, 12, 13, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30,
];

fn bad(n: i32, what: &str) -> Error {
    Error::format(format!("rule {n}: {what}"))
}

fn range(n: i32, what: &str, v: i32, lo: i32, hi: i32) -> Result<()> {
    if (lo..=hi).contains(&v) {
        Ok(())
    } else {
        Err(bad(n, &format!("{what} must be {lo}-{hi} (it is {v})")))
    }
}

/// Checks that the packets of a rule are what ElkRP writes: WHENEVER in the first packet, every AND in a following packet
/// whose result half says "more follows" (type 1), one THEN in the packet that ends the AND chain, further THENs in packets
/// that start with type 18, a FOR packet (type 19) only straight after a THEN packet, and item numbers in range.
pub fn validate_rule(r: &RuleDto, text_packets: usize) -> Result<()> {
    let n = r.n;
    let pk: Vec<[u8; 8]> = r
        .packets
        .iter()
        .map(|p| {
            let mut x = [0u8; 8];
            for (i, v) in p.iter().enumerate().take(8) {
                x[i] = *v as u8;
            }
            x
        })
        .collect();
    let first = &pk[0];
    if !TRIGGER_TYPES.contains(&ptype(first)) {
        return Err(bad(
            n,
            &format!(
                "the first packet is not a WHENEVER trigger (packet type {})",
                ptype(first)
            ),
        ));
    }
    check_trigger(n, first)?;
    let mut p = 0;
    while rtype(&pk[p]) == 1 {
        p += 1;
        if p >= pk.len() || matches!(ptype(&pk[p]), 18 | 19) {
            return Err(bad(n, "an AND is not followed by a THEN"));
        }
        if !CONDITION_TYPES.contains(&ptype(&pk[p])) {
            return Err(bad(
                n,
                &format!(
                    "AND packet {} has an unknown condition type {}",
                    p + 1,
                    ptype(&pk[p])
                ),
            ));
        }
        check_condition(n, &pk[p])?;
    }
    let mut i = p;
    loop {
        if i != p && ptype(&pk[i]) != 18 {
            return Err(bad(
                n,
                &format!(
                    "packet {} must be an additional THEN (type 18) or a FOR (type 19)",
                    i + 1
                ),
            ));
        }
        let rt = rtype(&pk[i]);
        if !RESULT_TYPES.contains(&rt) {
            return Err(bad(
                n,
                &format!("packet {} has no valid THEN (result type {rt})", i + 1),
            ));
        }
        check_result(n, &pk[i], text_packets)?;
        i += 1;
        if i < pk.len() && ptype(&pk[i]) == 19 {
            if rt != 4 {
                return Err(bad(n, "a FOR packet only belongs to a lighting THEN"));
            }
            let f = &pk[i];
            if f[1] == 0 {
                let secs = u32::from_be_bytes([f[4], f[5], f[6], f[7]]);
                if secs == 0 {
                    return Err(bad(n, "a FOR needs a time or a custom value"));
                }
            } else if !(1..=32).contains(&f[1]) {
                return Err(bad(n, "the FOR custom value must be 1-32"));
            }
            i += 1;
        }
        if i >= pk.len() {
            break;
        }
    }
    Ok(())
}

fn check_trigger(n: i32, p: &[u8; 8]) -> Result<()> {
    match ptype(p) {
        2 if p[0] & 7 == 7 => {
            if p[2] == 0 && p[3] == 0 {
                return Err(bad(n, "EVERY needs hours or minutes"));
            }
            range(n, "minutes", p[3] as i32, 0, 59)
        }
        2 => {
            if p[1] > 0 {
                range(n, "custom value", (p[1] & 0x7F) as i32, 1, 32)
            } else {
                range(n, "hour", p[2] as i32, 0, 23)?;
                range(n, "minute", p[3] as i32, 0, 59)
            }
        }
        3 if p[2] & 0x80 == 0 => range(n, "hour", p[1] as i32, 0, 23),
        4 if p[2] & 0x80 == 0 => range(n, "minute", p[1] as i32, 0, 59),
        11 => {
            let ev = p[2] as i32 * 256 + p[3] as i32;
            if event_table_present() && event_names().get(ev).is_none() {
                return Err(bad(n, &format!("event {ev} is not in ElkRP's event list")));
            }
            Ok(())
        }
        12 => {
            range(n, "zone", p[3] as i32, 1, 208)?;
            if p[2] & 0x80 == 0 {
                range(n, "zone state", p[1] as i32, 0, 5)?;
            }
            Ok(())
        }
        16 | 23 => range(n, "counter", p[1] as i32, 1, 64),
        21 => range(n, "thermostat", p[1] as i32, 1, 16),
        _ => Ok(()),
    }
}

fn check_condition(n: i32, p: &[u8; 8]) -> Result<()> {
    match ptype(p) {
        2 => {
            if p[1] > 0 {
                range(n, "custom value", (p[1] & 0x7F) as i32, 1, 32)
            } else {
                range(n, "hour", p[2] as i32, 0, 23)?;
                range(n, "minute", p[3] as i32, 0, 59)
            }
        }
        6 if p[2] & 0x80 == 0 && p[0] & 7 != 7 => range(n, "month", p[1] as i32, 1, 12),
        7 if p[2] & 0x80 == 0 => range(n, "day of the month", p[1] as i32, 1, 31),
        8 if p[2] & 0x80 == 0 => range(n, "year (2000 + value)", p[1] as i32, 0, 99),
        11 => {
            let ev = p[2] as i32 * 256 + p[3] as i32;
            if event_table_present() && event_names().get(ev).is_none() {
                return Err(bad(n, &format!("event {ev} is not in ElkRP's event list")));
            }
            Ok(())
        }
        12 => {
            range(n, "zone", p[3] as i32, 1, 208)?;
            if p[2] & 0x80 == 0 {
                range(n, "zone state", p[1] as i32, 0, 5)?;
            }
            Ok(())
        }
        16 | 23 => range(n, "counter", p[1] as i32, 1, 64),
        17 => range(n, "user", p[3] as i32, 1, 255),
        21 => range(n, "thermostat", p[1] as i32, 1, 16),
        _ => Ok(()),
    }
}

fn check_result(n: i32, p: &[u8; 8], text_packets: usize) -> Result<()> {
    let v = |x: u8| x as i32;
    match rtype(p) {
        2 => {
            range(n, "arming mode", v(p[4] & 7), 0, 6)?;
            if p[5] == 0 {
                return Err(bad(n, "pick at least one area to arm or disarm"));
            }
            Ok(())
        }
        3 if p[5] != 255 => {
            range(n, "output", v(p[5]), 1, 64)?;
            if p[6] & 0x80 != 0 {
                range(n, "FOR custom value", v(p[7] & 0x7F), 1, 32)?;
            }
            Ok(())
        }
        4 => {
            if ![1, 2, 3, 4, 5, 6, 7, 8, 9, 16].contains(&p[6]) {
                return Err(bad(
                    n,
                    &format!("lighting command {} is not one ElkRP knows", p[6]),
                ));
            }
            if matches!(p[6], 5 | 6) {
                range(n, "dim / brighten steps", v(p[7]), 1, 255)?;
            }
            Ok(())
        }
        5 => {
            let w = p[6] as i32 * 256 + p[7] as i32;
            range(n, "announce zone or voice message", w, 1, 208 + 115)
        }
        7 | 8 => {
            if rtype(p) == 8 && p[5] == 0 && p[6] == 0 && p[7] == 0 {
                return Ok(());
            }
            let ptr = (p[6] & 7) as usize * 256 + p[7] as usize;
            let first = PACKET_COUNT - text_packets;
            if ptr < first || ptr >= PACKET_COUNT {
                return Err(bad(n, "the text string it points at does not exist"));
            }
            Ok(())
        }
        9 => range(n, "counter", v(p[5]), 1, 64),
        10 => range(n, "task", v(p[5]), 1, 32),
        17 => range(n, "chime setting", v(p[5]), 0, 3),
        19 => range(n, "task", v(p[7]), 1, 32),
        20 => range(n, "output", v(p[7]), 1, 64),
        22 => range(n, "thermostat", v(p[6]), 1, 16),
        24 => range(n, "user", v(p[7]), 1, 255),
        25 => range(n, "counter", v(p[7]), 1, 64),
        _ => Ok(()),
    }
}
