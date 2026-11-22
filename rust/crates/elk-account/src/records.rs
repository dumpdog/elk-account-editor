//! The few record helpers this crate needs (private copies of what Records.cs holds; `elk-core` has the full set).

/// Zone record: 32 bytes. 0 function | 1 flag1 | 2 flag2 | 3 unused | 4..19 name | 20..31 six voice words.
#[derive(Debug, Clone, Default)]
pub struct ZoneRecord {
    pub bytes: [u8; 32],
}

impl ZoneRecord {
    pub const SIZE: usize = 32;

    pub fn from_bytes(data: &[u8]) -> ZoneRecord {
        let mut z = ZoneRecord::default();
        let n = data.len().min(Self::SIZE);
        z.bytes[..n].copy_from_slice(&data[..n]);
        z
    }
    pub fn function(&self) -> u8 {
        self.bytes[0]
    }
    pub fn set_function(&mut self, v: u8) {
        self.bytes[0] = v;
    }
    pub fn flag1(&self) -> u8 {
        self.bytes[1]
    }
    pub fn set_flag1(&mut self, v: u8) {
        self.bytes[1] = v;
    }
    pub fn flag2(&self) -> u8 {
        self.bytes[2]
    }
    pub fn set_flag2(&mut self, v: u8) {
        self.bytes[2] = v;
    }
    /// ASCII text of the name field (bytes above 127 read as '?'), trailing NUL and space removed.
    pub fn name(&self) -> String {
        ascii_field(&self.bytes, 4)
    }
    pub fn set_name(&mut self, v: &str) {
        name_field_put(&mut self.bytes, 4, v);
    }
    pub fn voice(&self) -> [u16; 6] {
        let mut v = [0u16; 6];
        for (i, w) in v.iter_mut().enumerate() {
            *w = ((self.bytes[20 + 2 * i] as u16) << 8) | self.bytes[21 + 2 * i] as u16;
        }
        v
    }
    pub fn crc(&self) -> u16 {
        crc16(&self.bytes)
    }
}

/// CRC-16, polynomial 0x1021, init 0, MSB first.
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u32 = 0;
    for &b in data {
        crc ^= (b as u32) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                ((crc << 1) ^ 0x1021) & 0xFFFF
            } else {
                (crc << 1) & 0xFFFF
            };
        }
    }
    crc as u16
}

/// Zone flag1 bits.
pub struct ZoneFlag1;

impl ZoneFlag1 {
    pub const BYPASSABLE: u8 = 1;
    pub const PERIODIC_TRIP: u8 = 2;
    pub const FAST_RESPONSE: u8 = 4;
    pub const FORCE_ARM: u8 = 8;
    pub const CHIME: u8 = 16;
    pub const CROSS_ZONED: u8 = 32;
    pub const USE_DIALER_DELAY: u8 = 64;
    pub const LISTEN_IN: u8 = 128;
}

/// Zone flag2: bits 0-2 zone type, bit 3 swinger shutdown, bits 4-6 area stored as (area - 1), bit 7 silent alarm.
pub struct ZoneFlag2;

impl ZoneFlag2 {
    pub const TYPE_MASK: u8 = 0x07;
    pub const SWINGER_SHUTDOWN: u8 = 0x08;
    pub const AREA_MASK: u8 = 0x70;
    pub const AREA_SHIFT: u8 = 4;
    pub const SILENT_ALARM: u8 = 0x80;

    pub fn type_of(flag2: u8) -> i32 {
        (flag2 & Self::TYPE_MASK) as i32
    }
    pub fn area_of(flag2: u8) -> i32 {
        (((flag2 & Self::AREA_MASK) >> Self::AREA_SHIFT) + 1) as i32
    }
    pub fn with(mut flag2: u8, ty: Option<i32>, area: Option<i32>) -> u8 {
        if let Some(t) = ty {
            flag2 = (flag2 & !Self::TYPE_MASK) | ((t & Self::TYPE_MASK as i32) as u8);
        }
        if let Some(a) = area {
            flag2 = (flag2 & !Self::AREA_MASK)
                | ((((a - 1) << Self::AREA_SHIFT) & Self::AREA_MASK as i32) as u8);
        }
        flag2
    }
}

fn ascii_field(bytes: &[u8], offset: usize) -> String {
    let s: String = bytes[offset..offset + 16]
        .iter()
        .map(|&b| if b < 128 { b as char } else { '?' })
        .collect();
    s.trim_end_matches(['\0', ' ']).to_string()
}

/// `NameField.Put`: 16-byte names keep their padding when the text is unchanged.
pub fn name_field_put(bytes: &mut [u8], offset: usize, value: &str) {
    let units: Vec<u16> = value.encode_utf16().collect();
    let text: String = if units.len() > 16 {
        String::from_utf16_lossy(&units[..16])
    } else {
        value.to_string()
    };
    if ascii_field(bytes, offset) == text {
        return;
    }
    let pad = if bytes[offset + 15] == b' ' { b' ' } else { 0 };
    for b in &mut bytes[offset..offset + 16] {
        *b = pad;
    }
    // Encoding.ASCII: anything above 127 becomes '?'
    let mut k = 0;
    let mut prev_high = false;
    for u in text.encode_utf16() {
        if (0xDC00..0xE000).contains(&u) && prev_high {
            prev_high = false;
            continue; // second half of a pair: one '?' was written
        }
        prev_high = (0xD800..0xDC00).contains(&u);
        if k < 16 {
            bytes[offset + k] = if u < 128 { u as u8 } else { b'?' };
        }
        k += 1;
    }
}
