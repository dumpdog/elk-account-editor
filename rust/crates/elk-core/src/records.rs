//! Port of Records.cs: zone, area and keypad records as stored and sent.

use crate::framing::{ascii_bytes, Crc16};

/// `Encoding.ASCII.GetString(bytes).TrimEnd('\0', ' ')`: bytes above 0x7F decode to `?`.
fn ascii_trimmed(bytes: &[u8]) -> Vec<u8> {
    let mut v: Vec<u8> = bytes.iter().map(|&b| if b < 0x80 { b } else { b'?' }).collect();
    while matches!(v.last(), Some(0) | Some(b' ')) {
        v.pop();
    }
    v
}

fn ascii_string(bytes: &[u8]) -> String {
    ascii_trimmed(bytes).into_iter().map(|b| b as char).collect()
}

/// 16-byte name fields. A panel pads them with spaces or NULs; rewriting an unchanged name must not alter the bytes
/// (that changes the record CRC).
pub(crate) struct NameField;

impl NameField {
    pub(crate) fn put(bytes: &mut [u8], offset: usize, value: &str) {
        // .NET strings are UTF-16: the cut at 16 is in code units, and every non-ASCII unit encodes as '?'.
        let units: Vec<u16> = value.encode_utf16().take(16).collect();
        let text: Vec<u8> = units.iter().map(|&u| if u < 0x80 { u as u8 } else { b'?' }).collect();
        // The C# compares decoded strings; a non-ASCII unit never equals the '?' it decodes to, which only matters for text containing
        // such units, so compare on the unit level.
        let existing = ascii_trimmed(&bytes[offset..offset + 16]);
        let same = existing.len() == units.len() && existing.iter().zip(&units).all(|(&e, &u)| e as u16 == u);
        if same {
            return; // unchanged: keep the existing padding
        }
        let pad = if bytes[offset + 15] == b' ' { b' ' } else { 0 }; // keep the field's existing padding style
        for b in &mut bytes[offset..offset + 16] {
            *b = pad;
        }
        bytes[offset..offset + text.len()].copy_from_slice(&text);
    }
}

/// Zone record as stored and sent: 32 bytes.
/// 0 function | 1 flag1 | 2 flag2 | 3 unused | 4..19 name (ASCII, NUL or space padded) | 20..31 six voice words (H,L).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneRecord {
    /// The raw record.
    pub bytes: [u8; ZoneRecord::SIZE],
}

impl Default for ZoneRecord {
    fn default() -> Self {
        ZoneRecord { bytes: [0; ZoneRecord::SIZE] }
    }
}

impl ZoneRecord {
    /// Record size in bytes.
    pub const SIZE: usize = 32;

    /// A zeroed record.
    pub fn new() -> Self {
        Self::default()
    }
    /// Byte 0.
    pub fn function(&self) -> u8 {
        self.bytes[0]
    }
    /// Sets byte 0.
    pub fn set_function(&mut self, v: u8) {
        self.bytes[0] = v;
    }
    /// Byte 1 (see [`ZoneFlag1`]).
    pub fn flag1(&self) -> u8 {
        self.bytes[1]
    }
    /// Sets byte 1.
    pub fn set_flag1(&mut self, v: u8) {
        self.bytes[1] = v;
    }
    /// Byte 2 (see [`ZoneFlag2`]).
    pub fn flag2(&self) -> u8 {
        self.bytes[2]
    }
    /// Sets byte 2.
    pub fn set_flag2(&mut self, v: u8) {
        self.bytes[2] = v;
    }
    /// Name, trailing NULs and spaces removed.
    pub fn name(&self) -> String {
        ascii_string(&self.bytes[4..20])
    }
    /// Sets the name, keeping the field's padding style (and the bytes when unchanged).
    pub fn set_name(&mut self, v: &str) {
        NameField::put(&mut self.bytes, 4, v);
    }
    /// The six voice words.
    pub fn voice(&self) -> [u16; 6] {
        let mut v = [0u16; 6];
        for (i, w) in v.iter_mut().enumerate() {
            *w = ((self.bytes[20 + 2 * i] as u16) << 8) | self.bytes[21 + 2 * i] as u16;
        }
        v
    }
    /// The "modified" CRC ElkRP keeps (ModCRC): CRC-16 over all 32 bytes.
    pub fn crc(&self) -> u16 {
        Crc16::compute(&self.bytes)
    }
    /// Copies up to 32 bytes of `data` into a new record.
    pub fn from_bytes(data: &[u8]) -> ZoneRecord {
        let mut z = ZoneRecord::new();
        let n = data.len().min(Self::SIZE);
        z.bytes[..n].copy_from_slice(&data[..n]);
        z
    }
}

/// Zone flag1 bits (a byte-sized flag set). UI labels in comments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ZoneFlag1(pub u8);

impl ZoneFlag1 {
    /// [Bypassable]
    pub const BYPASSABLE: ZoneFlag1 = ZoneFlag1(1);
    /// [Periodic trip] (ElkRP code calls it ActivityMon)
    pub const PERIODIC_TRIP: ZoneFlag1 = ZoneFlag1(2);
    /// [Fast loop response]
    pub const FAST_RESPONSE: ZoneFlag1 = ZoneFlag1(4);
    /// [Force armable]
    pub const FORCE_ARM: ZoneFlag1 = ZoneFlag1(8);
    /// [Enable chime]
    pub const CHIME: ZoneFlag1 = ZoneFlag1(16);
    /// [In cross zone pool]
    pub const CROSS_ZONED: ZoneFlag1 = ZoneFlag1(32);
    /// [Use dialer delay] (ElkRP code calls it Abort)
    pub const USE_DIALER_DELAY: ZoneFlag1 = ZoneFlag1(64);
    /// [Listen in]
    pub const LISTEN_IN: ZoneFlag1 = ZoneFlag1(128);

    /// True when every bit of `other` is set.
    pub fn contains(self, other: ZoneFlag1) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for ZoneFlag1 {
    type Output = ZoneFlag1;
    fn bitor(self, rhs: ZoneFlag1) -> ZoneFlag1 {
        ZoneFlag1(self.0 | rhs.0)
    }
}

/// Zone flag2: bits 0-2 zone type (hook-up), bit 3 swinger shutdown, bits 4-6 area stored as (area - 1), bit 7 silent alarm.
pub struct ZoneFlag2;

impl ZoneFlag2 {
    /// Mask of the zone type bits.
    pub const TYPE_MASK: u8 = 0x07;
    /// Swinger shutdown bit.
    pub const SWINGER_SHUTDOWN: u8 = 0x08;
    /// Mask of the area bits.
    pub const AREA_MASK: u8 = 0x70;
    /// Shift of the area bits.
    pub const AREA_SHIFT: u8 = 4;
    /// Silent alarm bit.
    pub const SILENT_ALARM: u8 = 0x80;

    /// Zone type (0-7).
    pub fn type_of(flag2: u8) -> i32 {
        (flag2 & Self::TYPE_MASK) as i32
    }
    /// Area (1-8).
    pub fn area(flag2: u8) -> i32 {
        (((flag2 & Self::AREA_MASK) >> Self::AREA_SHIFT) as i32) + 1
    }
    /// Returns `flag2` with the type and/or area replaced.
    pub fn with(mut flag2: u8, type_: Option<i32>, area: Option<i32>) -> u8 {
        if let Some(t) = type_ {
            flag2 = ((flag2 as i32 & !(Self::TYPE_MASK as i32)) | (t & Self::TYPE_MASK as i32)) as u8;
        }
        if let Some(a) = area {
            flag2 = ((flag2 as i32 & !(Self::AREA_MASK as i32)) | (((a - 1) << Self::AREA_SHIFT) & Self::AREA_MASK as i32)) as u8;
        }
        flag2
    }
}

/// Area record, 24 bytes.
/// 0..15 name | 16 exit1 | 17 entrance1 | 18 exit2 | 19 entrance2 | 20 flags1 | 21 flags2 | 22 arm time window | 23 unused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AreaRecord {
    /// The raw record.
    pub bytes: [u8; AreaRecord::SIZE],
}

impl Default for AreaRecord {
    fn default() -> Self {
        AreaRecord { bytes: [0; AreaRecord::SIZE] }
    }
}

impl AreaRecord {
    /// Record size in bytes.
    pub const SIZE: usize = 24;

    /// A zeroed record.
    pub fn new() -> Self {
        Self::default()
    }
    /// Name, trailing NULs and spaces removed.
    pub fn name(&self) -> String {
        ascii_string(&self.bytes[0..16])
    }
    /// Sets the name, keeping the field's padding style.
    pub fn set_name(&mut self, v: &str) {
        NameField::put(&mut self.bytes, 0, v);
    }
    /// Byte 16.
    pub fn exit1(&self) -> u8 {
        self.bytes[16]
    }
    /// Sets byte 16.
    pub fn set_exit1(&mut self, v: u8) {
        self.bytes[16] = v;
    }
    /// Byte 17.
    pub fn entrance1(&self) -> u8 {
        self.bytes[17]
    }
    /// Sets byte 17.
    pub fn set_entrance1(&mut self, v: u8) {
        self.bytes[17] = v;
    }
    /// Byte 18.
    pub fn exit2(&self) -> u8 {
        self.bytes[18]
    }
    /// Sets byte 18.
    pub fn set_exit2(&mut self, v: u8) {
        self.bytes[18] = v;
    }
    /// Byte 19.
    pub fn entrance2(&self) -> u8 {
        self.bytes[19]
    }
    /// Sets byte 19.
    pub fn set_entrance2(&mut self, v: u8) {
        self.bytes[19] = v;
    }
    /// Byte 20.
    pub fn flags1(&self) -> u8 {
        self.bytes[20]
    }
    /// Sets byte 20.
    pub fn set_flags1(&mut self, v: u8) {
        self.bytes[20] = v;
    }
    /// Byte 21.
    pub fn flags2(&self) -> u8 {
        self.bytes[21]
    }
    /// Sets byte 21.
    pub fn set_flags2(&mut self, v: u8) {
        self.bytes[21] = v;
    }
    /// Byte 22.
    pub fn arm_time_window(&self) -> u8 {
        self.bytes[22]
    }
    /// Sets byte 22.
    pub fn set_arm_time_window(&mut self, v: u8) {
        self.bytes[22] = v;
    }
    /// CRC-16 over all 24 bytes.
    pub fn crc(&self) -> u16 {
        Crc16::compute(&self.bytes)
    }
}

/// Keypad record, 148 bytes. Two stored CRCs: `crc1` over bytes 0..27, `crc2` over 28..147.
/// 0 area | 1..5 flags1-5 | 6..21 name | 22 LED state | 23..24 flags6-7 | 25..27 unused | 28..39 six F-key definitions (H,L) |
/// 40..51 six F-key light definitions (H,L) | 52..147 six F-key names, 16 chars each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeypadRecord {
    /// The raw record.
    pub bytes: [u8; KeypadRecord::SIZE],
}

impl Default for KeypadRecord {
    fn default() -> Self {
        KeypadRecord { bytes: [0; KeypadRecord::SIZE] }
    }
}

impl KeypadRecord {
    /// Record size in bytes.
    pub const SIZE: usize = 148;

    /// A zeroed record.
    pub fn new() -> Self {
        Self::default()
    }
    /// Byte 0.
    pub fn area(&self) -> u8 {
        self.bytes[0]
    }
    /// Sets byte 0.
    pub fn set_area(&mut self, v: u8) {
        self.bytes[0] = v;
    }
    /// Name, trailing NULs and spaces removed.
    pub fn name(&self) -> String {
        ascii_string(&self.bytes[6..22])
    }
    /// Sets the name, keeping the field's padding style.
    pub fn set_name(&mut self, v: &str) {
        NameField::put(&mut self.bytes, 6, v);
    }
    /// F-key definition word (`f` = 0..5).
    pub fn fkey_definition(&self, f: usize) -> u16 {
        ((self.bytes[28 + 2 * f] as u16) << 8) | self.bytes[29 + 2 * f] as u16
    }
    /// F-key name (`f` = 0..5).
    pub fn fkey_name(&self, f: usize) -> String {
        ascii_string(&self.bytes[52 + 16 * f..52 + 16 * f + 16])
    }
    /// CRC over bytes 0..27.
    pub fn crc1(&self) -> u16 {
        Crc16::compute(&self.bytes[..28])
    }
    /// CRC over bytes 28..147.
    pub fn crc2(&self) -> u16 {
        Crc16::compute(&self.bytes[28..])
    }
}

/// `Encoding.ASCII.GetBytes` for callers that fill name fields by hand.
pub fn ascii_name_bytes(s: &str) -> Vec<u8> {
    ascii_bytes(s)
}
