//! Port of PanelBackup.cs: a full copy of the panel's programming and its JSON file format.
//!
//! The JSON is written by hand to match System.Text.Json with `WriteIndented = true` and the default encoder byte for byte:
//! PascalCase property names, 2-space indent, `\r\n` line ends on Windows (`Environment.NewLine`; `\n` elsewhere), and
//! `"`, `'`, `&`, `<`, `>`, `+`, `` ` `` plus everything outside printable ASCII escaped as `\uXXXX`.

use crate::elk_session::ElkSession;
use crate::error::{hex_decode, hex_upper, ElkError, Result};
use crate::handshake::Section;
use crate::panel_auto::*;
use crate::records::{AreaRecord, KeypadRecord, ZoneRecord};
use serde::Deserialize;

/// A section item that is compared and restored byte for byte.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct BlockJson {
    /// Section id.
    #[serde(default)]
    pub section: u8,
    /// Item number.
    #[serde(default)]
    pub item: u8,
    /// Upper-case hex of the data.
    #[serde(default)]
    pub hex: String,
    /// Part (keypads, wireless blocks).
    #[serde(default)]
    pub part: u8,
}

/// An automation item or the rule / text packets as saved (not in the C# backup format; the C# ignores the member). `Kind` is an
/// [`crate::panel_auto::AutoKind`] label (`task`, `counter`, `thermostat`, `alarm duration`, `custom setting`, `output`, `light`,
/// `voice message`) with `Number` its number (1-based), or `packets` (number 0): all 528 rule and text packets, 4224 bytes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AutoBlockJson {
    /// Kind label.
    #[serde(default)]
    pub kind: String,
    /// Item number.
    #[serde(default)]
    pub number: u32,
    /// Upper-case hex of the data.
    #[serde(default)]
    pub hex: String,
}

/// A zone as saved.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ZoneJson {
    /// Zone number.
    #[serde(default)]
    pub number: u8,
    /// Name (applied over the hex on restore).
    #[serde(default)]
    pub name: String,
    /// Function byte.
    #[serde(default)]
    pub function: u8,
    /// Flag1 byte.
    #[serde(default)]
    pub flag1: u8,
    /// Flag2 byte.
    #[serde(default)]
    pub flag2: u8,
    /// Record hex.
    #[serde(default)]
    pub hex: String,
}

/// An area as saved.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AreaJson {
    /// Area number.
    #[serde(default)]
    pub number: u8,
    /// Name.
    #[serde(default)]
    pub name: String,
    /// Exit delay 1.
    #[serde(default)]
    pub exit1: u8,
    /// Entrance delay 1.
    #[serde(default)]
    pub entrance1: u8,
    /// Exit delay 2.
    #[serde(default)]
    pub exit2: u8,
    /// Entrance delay 2.
    #[serde(default)]
    pub entrance2: u8,
    /// Flags 1.
    #[serde(default)]
    pub flags1: u8,
    /// Flags 2.
    #[serde(default)]
    pub flags2: u8,
    /// Arm time window.
    #[serde(default)]
    pub arm_time_window: u8,
    /// Record hex.
    #[serde(default)]
    pub hex: String,
}

/// A keypad as saved.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct KeypadJson {
    /// Keypad number.
    #[serde(default)]
    pub number: u8,
    /// Name.
    #[serde(default)]
    pub name: String,
    /// Area.
    #[serde(default)]
    pub area: u8,
    /// Record hex.
    #[serde(default)]
    pub hex: String,
}

/// A full copy of the panel's programming (zones, areas, keypads and the other sections), in the JSON format the CLI's download/upload use.
/// In a restore, a record's name and settings fields are applied over its hex, so a copy can also be hand-edited.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PanelFile {
    /// Control firmware.
    #[serde(default)]
    pub firmware: Option<String>,
    /// Control serial number.
    #[serde(default)]
    pub serial: Option<String>,
    /// Zones.
    #[serde(default)]
    pub zones: Vec<ZoneJson>,
    /// Areas.
    #[serde(default)]
    pub areas: Vec<AreaJson>,
    /// Keypads.
    #[serde(default)]
    pub keypads: Vec<KeypadJson>,
    /// Other sections, compared byte for byte: users, telephones, globals, dawn/dusk and the report codes.
    #[serde(default)]
    pub blocks: Vec<BlockJson>,
    /// Automation items and the rule / text packets (only present in a backup made with them; written only when not empty).
    #[serde(default, rename = "AutomationBlocks")]
    pub automation: Vec<AutoBlockJson>,
}

fn nl() -> &'static str {
    if cfg!(windows) {
        "\r\n"
    } else {
        "\n"
    }
}

/// System.Text.Json string with `JavaScriptEncoder.Default`.
fn json_string(out: &mut String, s: &str) {
    use std::fmt::Write;
    out.push('"');
    for u in s.encode_utf16() {
        match u {
            0x22 => out.push_str("\\u0022"),
            0x27 => out.push_str("\\u0027"),
            0x26 => out.push_str("\\u0026"),
            0x3C => out.push_str("\\u003C"),
            0x3E => out.push_str("\\u003E"),
            0x2B => out.push_str("\\u002B"),
            0x60 => out.push_str("\\u0060"),
            0x5C => out.push_str("\\\\"),
            0x08 => out.push_str("\\b"),
            0x09 => out.push_str("\\t"),
            0x0A => out.push_str("\\n"),
            0x0C => out.push_str("\\f"),
            0x0D => out.push_str("\\r"),
            0x20..=0x7E => out.push(u as u8 as char),
            _ => {
                let _ = write!(out, "\\u{u:04X}");
            }
        }
    }
    out.push('"');
}

struct JsonObj<'a> {
    out: &'a mut String,
    indent: usize,
    first: bool,
}

impl<'a> JsonObj<'a> {
    fn begin(out: &'a mut String, indent: usize) -> Self {
        out.push('{');
        JsonObj { out, indent, first: true }
    }
    fn key(&mut self, name: &str) {
        if !self.first {
            self.out.push(',');
        }
        self.first = false;
        self.out.push_str(nl());
        self.out.push_str(&" ".repeat(self.indent + 2));
        json_string(self.out, name);
        self.out.push_str(": ");
    }
    fn num(&mut self, name: &str, v: u8) {
        self.key(name);
        self.out.push_str(&v.to_string());
    }
    fn str(&mut self, name: &str, v: &str) {
        self.key(name);
        json_string(self.out, v);
    }
    fn opt_str(&mut self, name: &str, v: &Option<String>) {
        self.key(name);
        match v {
            Some(s) => json_string(self.out, s),
            None => self.out.push_str("null"),
        }
    }
    fn array<T>(&mut self, name: &str, items: &[T], write: impl Fn(&mut String, usize, &T)) {
        self.key(name);
        if items.is_empty() {
            self.out.push_str("[]");
            return;
        }
        self.out.push('[');
        for (i, it) in items.iter().enumerate() {
            if i > 0 {
                self.out.push(',');
            }
            self.out.push_str(nl());
            self.out.push_str(&" ".repeat(self.indent + 4));
            write(self.out, self.indent + 4, it);
        }
        self.out.push_str(nl());
        self.out.push_str(&" ".repeat(self.indent + 2));
        self.out.push(']');
    }
    fn end(self) {
        self.out.push_str(nl());
        self.out.push_str(&" ".repeat(self.indent));
        self.out.push('}');
    }
}

impl PanelFile {
    /// The JSON text, equal to `JsonSerializer.Serialize(this, new() { WriteIndented = true })`.
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        let mut o = JsonObj::begin(&mut out, 0);
        o.opt_str("Firmware", &self.firmware);
        o.opt_str("Serial", &self.serial);
        o.array("Zones", &self.zones, |out, ind, z| {
            let mut o = JsonObj::begin(out, ind);
            o.num("Number", z.number);
            o.str("Name", &z.name);
            o.num("Function", z.function);
            o.num("Flag1", z.flag1);
            o.num("Flag2", z.flag2);
            o.str("Hex", &z.hex);
            o.end();
        });
        o.array("Areas", &self.areas, |out, ind, a| {
            let mut o = JsonObj::begin(out, ind);
            o.num("Number", a.number);
            o.str("Name", &a.name);
            o.num("Exit1", a.exit1);
            o.num("Entrance1", a.entrance1);
            o.num("Exit2", a.exit2);
            o.num("Entrance2", a.entrance2);
            o.num("Flags1", a.flags1);
            o.num("Flags2", a.flags2);
            o.num("ArmTimeWindow", a.arm_time_window);
            o.str("Hex", &a.hex);
            o.end();
        });
        o.array("Keypads", &self.keypads, |out, ind, k| {
            let mut o = JsonObj::begin(out, ind);
            o.num("Number", k.number);
            o.str("Name", &k.name);
            o.num("Area", k.area);
            o.str("Hex", &k.hex);
            o.end();
        });
        o.array("Blocks", &self.blocks, |out, ind, b| {
            let mut o = JsonObj::begin(out, ind);
            o.num("Section", b.section);
            o.num("Item", b.item);
            o.str("Hex", &b.hex);
            o.num("Part", b.part);
            o.end();
        });
        if !self.automation.is_empty() {
            o.array("AutomationBlocks", &self.automation, |out, ind, b| {
                let mut o = JsonObj::begin(out, ind);
                o.str("Kind", &b.kind);
                o.key("Number");
                o.out.push_str(&b.number.to_string());
                o.str("Hex", &b.hex);
                o.end();
            });
        }
        o.end();
        out
    }

    /// Parses a backup file (property names are case-sensitive PascalCase, unknown properties are ignored, like the C# defaults).
    pub fn from_json(json: &str) -> Result<PanelFile> {
        serde_json::from_str::<PanelFile>(json).map_err(|e| ElkError::InvalidData(format!("not a panel backup file: {e}")))
    }
}

/// Reads and restores a full backup.
pub struct PanelBackup;

/// Section, first item, last item. The panel does not enforce item ranges (it mirrors data past the end), so these are the known limits.
pub const BLOCKS: [(i32, i32, i32); 8] = [
    (1, 1, 199),  // user codes (28 bytes)
    (8, 1, 8),    // telephone numbers (88)
    (7, 1, 2),    // globals (48) and dawn/dusk (200)
    (9, 1, 8),    // area report codes (20)
    (10, 1, 199), // user report codes (4)
    (11, 1, 1),   // system report codes (24)
    (12, 1, 208), // zone report codes (4)
    (16, 1, 1),   // keypad panic report codes (96)
];

fn fill(dst: &mut [u8], what: &str, hex: &str) -> Result<()> {
    let b = hex_decode(hex)?;
    if b.len() > dst.len() {
        return Err(ElkError::Argument(format!("{what}: hex is longer than the record")));
    }
    dst[..b.len()].copy_from_slice(&b);
    Ok(())
}

impl PanelBackup {
    /// Label of a section in log messages.
    pub fn block_label(section: i32) -> String {
        match section {
            1 => "user".into(),
            7 => "globals/dawn-dusk".into(),
            8 => "telephone".into(),
            9 => "area report codes".into(),
            10 => "user report codes".into(),
            11 => "system report codes".into(),
            12 => "zone report codes".into(),
            15 => "wireless receiver".into(),
            16 => "keypad report codes".into(),
            _ => format!("section {section}"),
        }
    }

    /// Transfers a full backup read makes (208 zones, 8 areas, 16 keypads of two parts, every block and the three wireless blocks).
    pub fn estimate_transfers() -> i32 {
        208 + 8 + 2 * 16 + BLOCKS.iter().map(|b| b.2 - b.1 + 1).sum::<i32>() + 3
    }

    /// Transfers [`PanelBackup::update`] reads when comparing with a backup (a write adds one more for every record that differs).
    pub fn estimate_transfers_for(f: &PanelFile) -> i32 {
        let auto: i32 = f.automation.iter().map(|b| if b.kind == PACKETS_KIND { PACKET_REQUESTS_ALL as i32 } else { 1 }).sum();
        (f.zones.len() + f.areas.len() + 2 * f.keypads.len() + f.blocks.len()) as i32 + auto
    }

    /// Like [`PanelBackup::read`] and also the automation items (outputs, tasks, counters, custom values, lights, voice messages) and all 528 rule
    /// and text packets.
    pub fn read_with_automation(s: &mut ElkSession) -> Result<PanelFile> {
        let mut f = Self::read(s)?;
        let firmware = s.panel.as_ref().map(|p| p.firmware.clone()).unwrap_or_default();
        for (k, n, d) in read_all_items(s, &firmware)? {
            f.automation.push(AutoBlockJson { kind: k.label().into(), number: n as u32, hex: hex_upper(&d) });
        }
        let pk: Vec<u8> = read_all_packets(s)?.iter().flatten().copied().collect();
        f.automation.push(AutoBlockJson { kind: PACKETS_KIND.into(), number: 0, hex: hex_upper(&pk) });
        Ok(f)
    }

    /// Transfers [`PanelBackup::read_with_automation`] makes at most.
    pub fn estimate_transfers_with_automation() -> i32 {
        Self::estimate_transfers() + estimate_backup_transfers()
    }

    /// Reads the whole panel.
    pub fn read(s: &mut ElkSession) -> Result<PanelFile> {
        let mut f = PanelFile { firmware: s.panel.as_ref().map(|p| p.firmware.clone()), serial: s.panel.as_ref().map(|p| p.serial_hex.clone()), ..Default::default() };
        for n in 1..=208u8 {
            let z = s.read_zone(n)?;
            f.zones.push(ZoneJson { number: n, name: z.name(), function: z.function(), flag1: z.flag1(), flag2: z.flag2(), hex: hex_upper(&z.bytes) });
        }
        for n in 1..=8u8 {
            let a = s.read_area(n)?;
            f.areas.push(AreaJson {
                number: n,
                name: a.name(),
                exit1: a.exit1(),
                entrance1: a.entrance1(),
                exit2: a.exit2(),
                entrance2: a.entrance2(),
                flags1: a.flags1(),
                flags2: a.flags2(),
                arm_time_window: a.arm_time_window(),
                hex: hex_upper(&a.bytes),
            });
        }
        for n in 1..=16u8 {
            let k = s.read_keypad(n)?;
            f.keypads.push(KeypadJson { number: n, name: k.name(), area: k.area(), hex: hex_upper(&k.bytes) });
        }
        for (sec, from, to) in BLOCKS {
            for n in from..=to {
                let data = s.read_block(sec as u8, n as u8, 0)?;
                f.blocks.push(BlockJson { section: sec as u8, item: n as u8, hex: hex_upper(&data), part: 0 });
            }
        }
        for part in 0..3u8 {
            // wireless receiver blocks: section 15, the part selects the block, the item number is ignored
            let data = s.read_block(15, 1, part)?;
            f.blocks.push(BlockJson { section: 15, item: 1, hex: hex_upper(&data), part });
        }
        Ok(f)
    }

    /// Compares `f` with the panel and, when `apply` is set, writes what differs. Returns the number of records that differ.
    /// Zones and areas are compared by the panel's stored CRC, keypads and the other sections byte for byte.
    pub fn update(s: &mut ElkSession, f: &PanelFile, apply: bool, mut log: Option<&mut dyn FnMut(&str)>) -> Result<i32> {
        let mut changed = 0;
        let mut say = |m: String| {
            if let Some(l) = log.as_mut() {
                l(&m);
            }
        };
        for j in &f.zones {
            let mut z = ZoneRecord::new();
            fill(&mut z.bytes, "zone", &j.hex)?;
            z.set_name(&j.name);
            z.set_function(j.function);
            z.set_flag1(j.flag1);
            z.set_flag2(j.flag2);
            if s.query_crc(Section::Zone, j.number)? == z.crc() {
                continue;
            }
            changed += 1;
            say(format!("zone {}: '{}' differs from panel", j.number, j.name));
            if apply {
                s.write_record(Section::Zone, j.number, &z.bytes, 0)?;
            }
        }
        for j in &f.areas {
            let mut a = AreaRecord::new();
            fill(&mut a.bytes, "area", &j.hex)?;
            a.set_name(&j.name);
            a.set_exit1(j.exit1);
            a.set_entrance1(j.entrance1);
            a.set_exit2(j.exit2);
            a.set_entrance2(j.entrance2);
            a.set_flags1(j.flags1);
            a.set_flags2(j.flags2);
            a.set_arm_time_window(j.arm_time_window);
            if s.query_crc(Section::Area, j.number)? == a.crc() {
                continue;
            }
            changed += 1;
            say(format!("area {}: '{}' differs from panel", j.number, j.name));
            if apply {
                s.write_record(Section::Area, j.number, &a.bytes, 0)?;
            }
        }
        for j in &f.keypads {
            let mut k = KeypadRecord::new();
            fill(&mut k.bytes, "keypad", &j.hex)?;
            k.set_name(&j.name);
            k.set_area(j.area);
            let panel_k = s.read_keypad(j.number)?;
            if panel_k.bytes == k.bytes {
                continue;
            }
            changed += 1;
            say(format!("keypad {}: '{}' differs from panel", j.number, j.name));
            if apply {
                s.write_keypad(j.number, &k)?;
            }
        }
        for b in &f.blocks {
            let want = hex_decode(&b.hex)?;
            let have = s.read_block(b.section, b.item, b.part)?;
            if have == want {
                continue;
            }
            changed += 1;
            let part = if b.part > 0 || b.section == 15 { format!(" part {}", b.part) } else { String::new() };
            say(format!("{} {}{} differs from panel", Self::block_label(b.section as i32), b.item, part));
            if apply {
                s.write_block(b.section, b.item, &want, b.part)?;
            }
        }
        for b in &f.automation {
            let want = hex_decode(&b.hex)?;
            if b.kind == PACKETS_KIND {
                let have: Vec<u8> = read_all_packets(s)?.iter().flatten().copied().collect();
                if have == want {
                    continue;
                }
                changed += 1;
                say("rule and text packets differ from panel".to_string());
                if apply {
                    let mut pk = Vec::new();
                    for c in want.chunks(PACKET_LEN) {
                        let mut p = [0u8; PACKET_LEN];
                        if c.len() != PACKET_LEN {
                            return Err(ElkError::InvalidData("rule and text packets: length is not a multiple of 8".into()));
                        }
                        p.copy_from_slice(c);
                        pk.push(p);
                    }
                    write_all_packets(s, &pk)?;
                }
                continue;
            }
            let kind = AutoKind::from_label(&b.kind).ok_or_else(|| ElkError::InvalidData(format!("unknown automation block kind '{}'", b.kind)))?;
            let n = b.number as usize;
            if n == 0 || n > kind.count() {
                return Err(ElkError::InvalidData(format!("{} {}: number out of range", b.kind, b.number)));
            }
            let have = kind.read_range(s, n, n)?.remove(0);
            if have == want {
                continue;
            }
            changed += 1;
            say(format!("{} {} differs from panel", b.kind, b.number));
            if apply {
                kind.write_one(s, n, &want)?;
            }
        }
        Ok(changed)
    }
}

/// `Kind` of the automation block that holds all 528 rule and text packets.
pub const PACKETS_KIND: &str = "packets";
