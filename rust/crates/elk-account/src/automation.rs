//! Port of `Elk.Protocol/Automation.cs`: the automation part of an ElkRP "Export Account" file (tasks, counters, custom values,
//! outputs, lights, voice messages, rule packets, text strings and rule comments) that follows the report codes.
//! Layout (M1G, software 5.x), one value per line, no record numbers (the position is the number):
//!   Tasks 32 x 28, Counters 80 x 12 (64 counters + 16 thermostats), Custom values 32 x 16, Outputs 64 x 28, Lights 256 x 28,
//!   Voice messages 115 x 12, one line `rulePacketsUsed,textPacketsUsed`, 528 x 8 packet values, 528 comment lines.

#![allow(clippy::needless_range_loop)]

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Serialize};

use crate::error::{parse_int_loose, Error, Result};
use crate::rules::{self, RuleDto};

pub const TASK_COUNT: usize = 32;
pub const TASK_LEN: usize = 28;
pub const COUNTER_COUNT: usize = 80;
pub const COUNTER_LEN: usize = 12;
pub const CUSTOM_COUNT: usize = 32;
pub const CUSTOM_LEN: usize = 16;
pub const OUTPUT_COUNT: usize = 64;
pub const OUTPUT_LEN: usize = 28;
pub const LIGHT_COUNT: usize = 256;
pub const LIGHT_LEN: usize = 28;
pub const VOICE_COUNT: usize = 115;
pub const VOICE_LEN: usize = 12;
pub const PACKET_COUNT: usize = 528;
pub const PACKET_LEN: usize = 8;

/// Number of file lines the block occupies.
pub const LINE_COUNT: usize = TASK_COUNT * TASK_LEN
    + COUNTER_COUNT * COUNTER_LEN
    + CUSTOM_COUNT * CUSTOM_LEN
    + OUTPUT_COUNT * OUTPUT_LEN
    + LIGHT_COUNT * LIGHT_LEN
    + VOICE_COUNT * VOICE_LEN
    + 1
    + PACKET_COUNT * PACKET_LEN
    + PACKET_COUNT;

fn new_records(n: usize, len: usize) -> Vec<Vec<u8>> {
    vec![vec![0u8; len]; n]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationBlock {
    pub tasks: Vec<Vec<u8>>,
    pub counters: Vec<Vec<u8>>,
    pub customs: Vec<Vec<u8>>,
    pub outputs: Vec<Vec<u8>>,
    pub lights: Vec<Vec<u8>>,
    pub voice: Vec<Vec<u8>>,
    pub packets: Vec<[u8; PACKET_LEN]>,
    pub rule_packets_used: usize,
    pub text_packets_used: usize,
    pub comments: Vec<String>,
}

impl Default for AutomationBlock {
    fn default() -> Self {
        AutomationBlock {
            tasks: new_records(TASK_COUNT, TASK_LEN),
            counters: new_records(COUNTER_COUNT, COUNTER_LEN),
            customs: new_records(CUSTOM_COUNT, CUSTOM_LEN),
            outputs: new_records(OUTPUT_COUNT, OUTPUT_LEN),
            lights: new_records(LIGHT_COUNT, LIGHT_LEN),
            voice: new_records(VOICE_COUNT, VOICE_LEN),
            packets: vec![[0; PACKET_LEN]; PACKET_COUNT],
            rule_packets_used: 0,
            text_packets_used: 0,
            comments: vec![String::new(); PACKET_COUNT],
        }
    }
}

/// A canonical byte value line: digits only, no leading zeros, 0-255 (what `int.TryParse(NumberStyles.None)` plus the
/// `ToString()` comparison accept).
fn canon_byte(s: &str) -> Option<u8> {
    let b = s.as_bytes();
    if b.is_empty()
        || b.len() > 3
        || !b.iter().all(u8::is_ascii_digit)
        || (b.len() > 1 && b[0] == b'0')
    {
        return None;
    }
    s.parse::<u16>().ok().filter(|v| *v <= 255).map(|v| v as u8)
}

fn canon_i32(s: &str) -> Option<i32> {
    let b = s.as_bytes();
    if b.is_empty() || !b.iter().all(u8::is_ascii_digit) || (b.len() > 1 && b[0] == b'0') {
        return None;
    }
    s.parse::<i32>().ok()
}

impl AutomationBlock {
    /// Reads the block starting at line `at`, or returns None when the software is older than 5, the file has fewer lines than
    /// the block needs or a line is not a canonical byte value (the lines are then copied through unchanged).
    pub fn try_parse(l: &[String], at: usize, software_version: &str) -> Option<AutomationBlock> {
        let major = parse_int_loose(software_version.split('.').next().unwrap_or(""))?;
        if major < 5 {
            return None;
        }
        if at + LINE_COUNT > l.len() {
            return None;
        }
        let mut b = AutomationBlock::default();
        let mut p = at;
        fn fill(l: &[String], p: &mut usize, arr: &mut [Vec<u8>]) -> bool {
            for rec in arr.iter_mut() {
                for slot in rec.iter_mut() {
                    let Some(v) = canon_byte(&l[*p]) else {
                        return false;
                    };
                    *slot = v;
                    *p += 1;
                }
            }
            true
        }
        if !(fill(l, &mut p, &mut b.tasks)
            && fill(l, &mut p, &mut b.counters)
            && fill(l, &mut p, &mut b.customs)
            && fill(l, &mut p, &mut b.outputs)
            && fill(l, &mut p, &mut b.lights)
            && fill(l, &mut p, &mut b.voice))
        {
            return None;
        }
        let hdr: Vec<&str> = l[p].split(',').collect();
        if hdr.len() != 2 {
            return None;
        }
        let rp = canon_i32(hdr[0])?;
        let tp = canon_i32(hdr[1])?;
        if i64::from(rp) + i64::from(tp) > PACKET_COUNT as i64 {
            return None;
        }
        b.rule_packets_used = rp as usize;
        b.text_packets_used = tp as usize;
        p += 1;
        for pk in b.packets.iter_mut() {
            for slot in pk.iter_mut() {
                *slot = canon_byte(&l[p])?;
                p += 1;
            }
        }
        for c in b.comments.iter_mut() {
            *c = l[p].clone();
            p += 1;
        }
        Some(b)
    }

    pub fn emit(&self, out: &mut Vec<String>) {
        let mut put = |arr: &[Vec<u8>]| {
            for rec in arr {
                for v in rec {
                    out.push(v.to_string());
                }
            }
        };
        put(&self.tasks);
        put(&self.counters);
        put(&self.customs);
        put(&self.outputs);
        put(&self.lights);
        put(&self.voice);
        out.push(format!(
            "{},{}",
            self.rule_packets_used, self.text_packets_used
        ));
        for pk in &self.packets {
            out.extend(pk.iter().map(|v| v.to_string()));
        }
        out.extend(self.comments.iter().cloned());
    }
}

// ---- model ----------------------------------------------------------------------------------------------------------

fn six() -> Vec<i32> {
    vec![0; 6]
}

/// An output or a task: a 16-character name whose first byte has bit 7 set when the item is shown, and six voice words.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NamedItemDto {
    pub n: i32,
    pub name: String,
    pub show: bool,
    pub voice: Vec<i32>,
}

impl Default for NamedItemDto {
    fn default() -> Self {
        NamedItemDto {
            n: 0,
            name: String::new(),
            show: false,
            voice: six(),
        }
    }
}

/// A custom value. Items 1-12 are the alarm durations (fixed names), 13-32 the custom settings.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CustomDto {
    pub n: i32,
    #[serde(rename = "type")]
    pub type_: i32,
    pub value: i32,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LightDto {
    pub n: i32,
    pub id: String,
    pub name: String,
    pub show: bool,
    pub two_way: bool,
    #[serde(rename = "type")]
    pub type_: i32,
    pub format: i32,
    pub voice: Vec<i32>,
}

impl Default for LightDto {
    fn default() -> Self {
        LightDto {
            n: 0,
            id: String::new(),
            name: String::new(),
            show: false,
            two_way: false,
            type_: 0,
            format: 0,
            voice: six(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VoiceMsgDto {
    pub n: i32,
    pub words: Vec<i32>,
}

impl Default for VoiceMsgDto {
    fn default() -> Self {
        VoiceMsgDto { n: 0, words: six() }
    }
}

/// A custom text string. `index` is its 1-based position in the file; a new string has index 0.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TextDto {
    pub index: i32,
    pub text: String,
    pub packets: i32,
    pub used_by_rules: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AutomationDto {
    pub tasks: Vec<NamedItemDto>,
    /// `string[64]` in C#: entries may be null.
    pub counters: Vec<Option<String>>,
    /// `string[16]` in C#: entries may be null.
    pub thermostats: Vec<Option<String>>,
    pub customs: Vec<CustomDto>,
    pub outputs: Vec<NamedItemDto>,
    pub lights: Vec<LightDto>,
    pub voice_messages: Vec<VoiceMsgDto>,
    pub texts: Vec<TextDto>,
    pub rule_packets_used: i32,
    pub text_packets_used: i32,
    pub packets_free: i32,
    pub rules: Vec<RuleDto>,
}

impl Default for AutomationDto {
    fn default() -> Self {
        AutomationDto {
            tasks: Vec::new(),
            counters: vec![None; 64],
            thermostats: vec![None; 16],
            customs: Vec::new(),
            outputs: Vec::new(),
            lights: Vec::new(),
            voice_messages: Vec::new(),
            texts: Vec::new(),
            rule_packets_used: 0,
            text_packets_used: 0,
            packets_free: 0,
            rules: Vec::new(),
        }
    }
}

// ---- label tables ---------------------------------------------------------------------------------------------------

/// A dictionary that keeps the order of first insertion (what .NET's `Dictionary<int, string>` enumerates and serialises in).
#[derive(Debug, Clone, Default)]
pub struct OrderedMap {
    items: Vec<(i32, String)>,
    index: HashMap<i32, usize>,
}

impl OrderedMap {
    pub fn insert(&mut self, k: i32, v: String) {
        match self.index.get(&k) {
            Some(&i) => self.items[i].1 = v,
            None => {
                self.index.insert(k, self.items.len());
                self.items.push((k, v));
            }
        }
    }
    pub fn get(&self, k: i32) -> Option<&str> {
        self.index.get(&k).map(|&i| self.items[i].1.as_str())
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn iter(&self) -> impl Iterator<Item = (i32, &str)> {
        self.items.iter().map(|(k, v)| (*k, v.as_str()))
    }
}

impl Serialize for OrderedMap {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(self.items.len()))?;
        for (k, v) in &self.items {
            m.serialize_entry(&k.to_string(), v)?;
        }
        m.end()
    }
}

pub struct AutomationMeta;

static WORDS_TSV: &str = include_str!("data/words-m1g.tsv");
static VOICE_TSV: &str = include_str!("data/voice-messages-m1g.tsv");
static OUTPUT_TSV: &str = include_str!("data/output-list-m1g.tsv");
/// The event table (ElkRP wording). Empty in builds made without `src/data/event-list-m1g.tsv` (see `build.rs`).
pub(crate) static EVENT_TSV: &str = include_str!(concat!(env!("OUT_DIR"), "/event-list-m1g.tsv"));

impl AutomationMeta {
    pub const CUSTOM_TYPES: [&'static str; 3] = ["Number", "Timer", "Time of day"];
    pub const LIGHT_TYPES: [&'static str; 8] = [
        "Appliance",
        "On/off switch",
        "Dimmer",
        "(3)",
        "(4)",
        "(5)",
        "(6)",
        "Other",
    ];
    pub const LIGHT_FORMATS: [&'static str; 8] = [
        "Standard",
        "Extended",
        "Preset Dim",
        "Compose",
        "Serial Expander",
        "(5)",
        "(6)",
        "(7)",
    ];
    pub const DURATION_NAMES: [&'static str; 12] = [
        "BurgDuration",
        "FireDuration",
        "Med Duration",
        "Pol Duration",
        "CO Duration",
        "EmerDuration",
        "FrezDuration",
        "Gas Duration",
        "HeatDuration",
        "WatrDuration",
        "Aux1Duration",
        "Aux2Duration",
    ];

    /// `LoadLines`: the text split on `\n`, empty entries dropped.
    pub(crate) fn load_lines(text: &'static str) -> impl Iterator<Item = &'static str> {
        text.split('\n').filter(|l| !l.is_empty())
    }

    fn load_pairs(text: &'static str) -> OrderedMap {
        let mut d = OrderedMap::default();
        for line in Self::load_lines(text) {
            let t: Vec<&str> = line.trim_end_matches('\r').split('\t').collect();
            if t.len() >= 2 {
                if let Some(k) = parse_int_loose(t[0]) {
                    d.insert(k, t[1].to_string());
                }
            }
        }
        d
    }

    /// ElkRP's vocabulary (WordLists, list version 0.8): word id to text.
    pub fn words() -> &'static OrderedMap {
        static M: OnceLock<OrderedMap> = OnceLock::new();
        M.get_or_init(|| Self::load_pairs(WORDS_TSV))
    }
    /// The 115 voice messages' descriptions by message number.
    pub fn voice_message_names() -> &'static OrderedMap {
        static M: OnceLock<OrderedMap> = OnceLock::new();
        M.get_or_init(|| Self::load_pairs(VOICE_TSV))
    }
    pub fn output_names() -> &'static OrderedMap {
        static M: OnceLock<OrderedMap> = OnceLock::new();
        M.get_or_init(|| Self::load_pairs(OUTPUT_TSV))
    }
}

// ---- codec ----------------------------------------------------------------------------------------------------------

/// Text of a name field: bit 7 of every byte is a flag, not part of the character.
pub fn name_at(b: &[u8], at: usize, len: usize) -> String {
    let s: String = b[at..at + len]
        .iter()
        .map(|&c| (c & 0x7F) as char)
        .collect();
    s.trim_end_matches(['\0', ' ']).to_string()
}

pub fn show_at(b: &[u8], at: usize) -> bool {
    b[at] & 0x80 != 0
}

/// Writes a name and its show flag. Nothing is touched when both are unchanged, so ElkRP's own padding survives.
pub fn put_name(b: &mut [u8], at: usize, len: usize, name: &str, show: Option<bool>) -> Result<()> {
    if name.encode_utf16().count() > len
        || name.chars().any(|c| (c as u32) < 32 || (c as u32) > 126)
    {
        return Err(Error::format(format!(
            "name must be at most {len} printable ASCII characters"
        )));
    }
    let old_show = show_at(b, at);
    let new_show = show.unwrap_or(old_show);
    if name_at(b, at, len) == name.trim_end_matches(' ') && old_show == new_show {
        return Ok(());
    }
    b[at..at + len].iter_mut().for_each(|x| *x = 0);
    for (i, c) in name.chars().enumerate() {
        b[at + i] = c as u8;
    }
    if new_show {
        b[at] |= 0x80;
    }
    Ok(())
}

pub fn words(b: &[u8], at: usize) -> Vec<i32> {
    (0..6)
        .map(|i| b[at + 2 * i] as i32 * 256 + b[at + 2 * i + 1] as i32)
        .collect()
}

pub fn put_words(b: &mut [u8], at: usize, w: &[i32], what: &str) -> Result<()> {
    for i in 0..6 {
        let v = w.get(i).copied().unwrap_or(0);
        if !(0..=65535).contains(&v) {
            return Err(Error::format(format!("{what}: voice word must be 0-65535")));
        }
        if b[at + 2 * i] as i32 * 256 + b[at + 2 * i + 1] as i32 == v {
            continue;
        }
        b[at + 2 * i] = (v >> 8) as u8;
        b[at + 2 * i + 1] = v as u8;
    }
    Ok(())
}

/// House letter and unit of light `n` (1-256), e.g. "A1".
pub fn light_id(n: usize) -> String {
    format!(
        "{}{}",
        (b'A' + ((n - 1) / 16) as u8) as char,
        (n - 1) % 16 + 1
    )
}

pub struct AutomationCodec;

impl AutomationCodec {
    pub fn from(a: &AutomationBlock) -> AutomationDto {
        let mut d = AutomationDto {
            rule_packets_used: a.rule_packets_used as i32,
            text_packets_used: a.text_packets_used as i32,
            packets_free: (PACKET_COUNT - a.rule_packets_used - a.text_packets_used) as i32,
            ..Default::default()
        };
        for i in 0..TASK_COUNT {
            d.tasks.push(NamedItemDto {
                n: i as i32 + 1,
                name: name_at(&a.tasks[i], 0, 16),
                show: show_at(&a.tasks[i], 0),
                voice: words(&a.tasks[i], 16),
            });
        }
        for i in 0..64 {
            d.counters[i] = Some(name_at(&a.counters[i], 0, 12));
        }
        for i in 0..16 {
            d.thermostats[i] = Some(name_at(&a.counters[64 + i], 0, 12));
        }
        for i in 0..CUSTOM_COUNT {
            let c = &a.customs[i];
            d.customs.push(CustomDto {
                n: i as i32 + 1,
                type_: c[0] as i32,
                value: ((c[1] as i32) << 16) | ((c[2] as i32) << 8) | c[3] as i32,
                name: name_at(c, 4, 12),
            });
        }
        for i in 0..OUTPUT_COUNT {
            d.outputs.push(NamedItemDto {
                n: i as i32 + 1,
                name: name_at(&a.outputs[i], 0, 16),
                show: show_at(&a.outputs[i], 0),
                voice: words(&a.outputs[i], 16),
            });
        }
        for i in 0..LIGHT_COUNT {
            let l = &a.lights[i];
            d.lights.push(LightDto {
                n: i as i32 + 1,
                id: light_id(i + 1),
                name: name_at(l, 1, 15),
                show: show_at(l, 1),
                two_way: l[0] & 2 != 0,
                type_: ((l[0] >> 2) & 7) as i32,
                format: ((l[0] >> 5) & 7) as i32,
                voice: words(l, 16),
            });
        }
        for i in 0..VOICE_COUNT {
            d.voice_messages.push(VoiceMsgDto {
                n: i as i32 + 1,
                words: words(&a.voice[i], 0),
            });
        }
        let starts = text_starts(a);
        let strs = text_strings(a);
        d.rules = rules::decode(a);
        let refs = rules::text_refs(a);
        for i in 0..strs.len() {
            let next = starts.get(i + 1).copied().unwrap_or(PACKET_COUNT);
            d.texts.push(TextDto {
                index: i as i32 + 1,
                text: strs[i].clone(),
                packets: (next - starts[i]) as i32,
                used_by_rules: i32::from(refs.contains(&starts[i])),
            });
        }
        d
    }

    /// Writes the model into the block. Only fields whose value changed are rewritten.
    pub fn apply(d: &AutomationDto, a: &mut AutomationBlock) -> Result<()> {
        for t in &d.tasks {
            if !(1..=TASK_COUNT as i32).contains(&t.n) {
                return Err(Error::format(format!("task {}: number must be 1-32", t.n)));
            }
            let b = &mut a.tasks[t.n as usize - 1];
            put_name(b, 0, 16, &t.name, Some(t.show))?;
            put_words(b, 16, &t.voice, &format!("task {}", t.n))?;
        }
        for o in &d.outputs {
            if !(1..=OUTPUT_COUNT as i32).contains(&o.n) {
                return Err(Error::format(format!(
                    "output {}: number must be 1-64",
                    o.n
                )));
            }
            let b = &mut a.outputs[o.n as usize - 1];
            put_name(b, 0, 16, &o.name, Some(o.show))?;
            put_words(b, 16, &o.voice, &format!("output {}", o.n))?;
        }
        for i in 0..64 {
            if let Some(Some(s)) = d.counters.get(i) {
                put_name(&mut a.counters[i], 0, 12, s, None)?;
            }
        }
        for i in 0..16 {
            if let Some(Some(s)) = d.thermostats.get(i) {
                put_name(&mut a.counters[64 + i], 0, 12, s, None)?;
            }
        }
        for c in &d.customs {
            if !(1..=CUSTOM_COUNT as i32).contains(&c.n) {
                return Err(Error::format(format!(
                    "custom value {}: number must be 1-32",
                    c.n
                )));
            }
            if !(0..=2).contains(&c.type_) {
                return Err(Error::format(format!(
                    "custom value {}: type must be 0 (number), 1 (timer) or 2 (time of day)",
                    c.n
                )));
            }
            if !(0..=0xFFFFFF).contains(&c.value) {
                return Err(Error::format(format!(
                    "custom value {}: value must be 0-16777215",
                    c.n
                )));
            }
            if (c.type_ == 2 && ((c.value >> 8) & 0xFF) > 23)
                || (c.type_ == 2 && (c.value & 0xFF) > 59)
            {
                return Err(Error::format(format!(
                    "custom value {}: a time of day needs hour 0-23 and minute 0-59",
                    c.n
                )));
            }
            let b = &mut a.customs[c.n as usize - 1];
            b[0] = c.type_ as u8;
            b[1] = (c.value >> 16) as u8;
            b[2] = (c.value >> 8) as u8;
            b[3] = c.value as u8;
            if c.n > 12 {
                put_name(b, 4, 12, &c.name, None)?; // the alarm durations have fixed names
            }
        }
        for l in &d.lights {
            if !(1..=LIGHT_COUNT as i32).contains(&l.n) {
                return Err(Error::format(format!(
                    "light {}: number must be 1-256",
                    l.n
                )));
            }
            if !(0..=7).contains(&l.type_) || !(0..=7).contains(&l.format) {
                return Err(Error::format(format!(
                    "light {}: type and format must be 0-7",
                    l.n
                )));
            }
            let b = &mut a.lights[l.n as usize - 1];
            b[0] = (b[0] & 0x01)
                | if l.two_way { 2 } else { 0 }
                | ((l.type_ as u8) << 2)
                | ((l.format as u8) << 5);
            put_name(b, 1, 15, &l.name, Some(l.show))?;
            put_words(b, 16, &l.voice, &format!("light {}", l.n))?;
        }
        for v in &d.voice_messages {
            if !(1..=VOICE_COUNT as i32).contains(&v.n) {
                return Err(Error::format(format!(
                    "voice message {}: number must be 1-115",
                    v.n
                )));
            }
            put_words(
                &mut a.voice[v.n as usize - 1],
                0,
                &v.words,
                &format!("voice message {}", v.n),
            )?;
        }
        // texts: work out the new packing first so the rules can be checked against the space that is really free
        let cur = text_strings(a);
        let cur_starts = text_starts(a);
        let texts_changed = d.texts.len() != cur.len()
            || d.texts
                .iter()
                .enumerate()
                .any(|(i, t)| t.index != i as i32 + 1 || t.text != cur[i]);
        let mut packed: Option<Vec<[u8; PACKET_LEN]>> = None;
        if texts_changed {
            for t in &d.texts {
                if t.index < 0 || t.index > cur.len() as i32 {
                    return Err(Error::format(format!(
                        "text string {} does not exist",
                        t.index
                    )));
                }
            }
            let mut seen = HashSet::new();
            for t in d.texts.iter().filter(|t| t.index > 0) {
                if !seen.insert(t.index) {
                    return Err(Error::format("a text string appears twice"));
                }
            }
            let refs = rules::text_refs(a);
            for i in 0..cur.len() {
                if d.texts.iter().all(|t| t.index != i as i32 + 1) && refs.contains(&cur_starts[i])
                {
                    return Err(Error::format(format!(
                        "text string {} is used by a rule and cannot be deleted",
                        i + 1
                    )));
                }
            }
            packed = Some(pack_texts(d.texts.iter().map(|t| t.text.as_str()))?);
        }
        let text_packets = packed.as_ref().map_or(a.text_packets_used, |p| p.len());
        rules::apply(&d.rules, a, text_packets)?;
        if let Some(p) = packed {
            repack_texts(&d.texts, &p, &cur_starts, a)?;
        }
        Ok(())
    }
}

// ---- text strings (the packets at the end of the rule area) -----------------------------------------------------------

/// The packet number where each text string starts, given the packets and how many of them (at the end) hold text.
pub fn text_starts_in(packets: &[[u8; PACKET_LEN]], text_used: usize) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut pkt = PACKET_COUNT - text_used;
    while pkt < PACKET_COUNT {
        starts.push(pkt);
        let mut esc = 0;
        let mut done = false;
        while pkt < PACKET_COUNT && !done {
            for &v in packets[pkt].iter() {
                if esc > 0 {
                    esc -= 1;
                } else if v == 0xFF {
                    esc = 3;
                } else if v == 0 {
                    done = true;
                    break;
                }
            }
            pkt += 1;
        }
    }
    starts
}

pub fn text_starts(a: &AutomationBlock) -> Vec<usize> {
    text_starts_in(&a.packets, a.text_packets_used)
}

pub fn text_strings_in(packets: &[[u8; PACKET_LEN]], text_used: usize) -> Vec<String> {
    let starts = text_starts_in(packets, text_used);
    let mut list = Vec::new();
    for s in 0..starts.len() {
        let end = starts.get(s + 1).copied().unwrap_or(PACKET_COUNT);
        let bytes: Vec<u8> = packets[starts[s]..end].iter().flatten().copied().collect();
        let mut sb = String::new();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == 0 {
                break;
            }
            if bytes[i] == 0xFF {
                let g = |k: usize| bytes.get(i + k).copied().unwrap_or(0);
                sb.push_str(&format!("{{ff:{:02x}:{:02x}:{:02x}}}", g(1), g(2), g(3)));
                i += 3;
            } else if (32..127).contains(&bytes[i]) {
                sb.push(bytes[i] as char);
            } else {
                sb.push('?');
            }
            i += 1;
        }
        list.push(sb);
    }
    list
}

pub fn text_strings(a: &AutomationBlock) -> Vec<String> {
    text_strings_in(&a.packets, a.text_packets_used)
}

fn is_net_white(u: u16) -> bool {
    (9..=13).contains(&u) || u == 32
}

/// `byte.TryParse(span, NumberStyles.HexNumber, Invariant)` for a two-unit span.
fn parse_hex_byte(s: &[u16]) -> Option<u8> {
    let mut i = 0;
    while i < s.len() && is_net_white(s[i]) {
        i += 1;
    }
    let start = i;
    while i < s.len() && char::from_u32(s[i] as u32).is_some_and(|c| c.is_ascii_hexdigit()) {
        i += 1;
    }
    if i == start {
        return None;
    }
    let digits: String = s[start..i].iter().map(|&u| u as u8 as char).collect();
    while i < s.len() && is_net_white(s[i]) {
        i += 1;
    }
    if s[i..].iter().any(|&u| u != 0) {
        return None;
    }
    u8::from_str_radix(&digits, 16).ok()
}

/// Packs strings into packets (each NUL terminated, from a packet boundary). `{ff:xx:xx:xx}` becomes the escape. Max 79 bytes.
pub fn pack_texts<'a>(texts: impl IntoIterator<Item = &'a str>) -> Result<Vec<[u8; PACKET_LEN]>> {
    let mut packets = Vec::new();
    for t in texts {
        let t: Vec<u16> = t.encode_utf16().collect();
        let eq = |at: usize, s: &str| s.encode_utf16().enumerate().all(|(k, c)| t[at + k] == c);
        let mut bytes: Vec<u8> = Vec::new();
        let mut i = 0;
        while i < t.len() {
            if t[i] == b'{' as u16 && i + 14 <= t.len() && eq(i, "{ff:") {
                let c = |k: usize| t[i + k];
                if c(6) == b':' as u16 && c(9) == b':' as u16 && c(12) == b'}' as u16 {
                    if let (Some(x1), Some(x2), Some(x3)) = (
                        parse_hex_byte(&t[i + 4..i + 6]),
                        parse_hex_byte(&t[i + 7..i + 9]),
                        parse_hex_byte(&t[i + 10..i + 12]),
                    ) {
                        bytes.extend([0xFF, x1, x2, x3]);
                        i += 13;
                        continue;
                    }
                }
            }
            if t[i] < 32 || t[i] > 126 {
                return Err(Error::format(
                    "text strings may only contain printable ASCII characters",
                ));
            }
            bytes.push(t[i] as u8);
            i += 1;
        }
        if bytes.len() > 79 {
            return Err(Error::format("a text string can be at most 79 characters"));
        }
        bytes.push(0);
        while !bytes.len().is_multiple_of(8) {
            bytes.push(0);
        }
        for ch in bytes.chunks(8) {
            let mut p = [0u8; PACKET_LEN];
            p.copy_from_slice(ch);
            packets.push(p);
        }
    }
    Ok(packets)
}

/// Replaces the text strings; rule packets stay where they are, the text area grows or shrinks at the end and the rules'
/// text pointers follow their strings.
pub fn repack_texts(
    texts: &[TextDto],
    packed: &[[u8; PACKET_LEN]],
    old_starts: &[usize],
    a: &mut AutomationBlock,
) -> Result<()> {
    if a.rule_packets_used + packed.len() > PACKET_COUNT {
        return Err(Error::format(format!(
            "the text strings need {} packets but only {} are free (rules use {})",
            packed.len(),
            PACKET_COUNT - a.rule_packets_used,
            a.rule_packets_used
        )));
    }
    let mut map = HashMap::new();
    let first = PACKET_COUNT - packed.len();
    let mut at = first;
    for t in texts {
        let n = pack_texts([t.text.as_str()])?.len();
        if t.index > 0 {
            map.insert(old_starts[t.index as usize - 1], at);
        }
        at += n;
    }
    rules::remap_text_refs(a, &map);
    for i in a.rule_packets_used..PACKET_COUNT {
        a.packets[i] = [0; PACKET_LEN];
    }
    for (i, p) in packed.iter().enumerate() {
        a.packets[first + i] = *p;
    }
    a.text_packets_used = packed.len();
    Ok(())
}
