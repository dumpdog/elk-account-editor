//! Port of DefaultRecords.cs (and `ZoneVoice` from ZoneImport.cs, which the zone default needs).

use crate::error::{ElkError, Result};
use crate::records::ZoneRecord;

/// Default zone voice description, as built by ElkRP (see ZoneImport.cs `ZoneVoice`).
pub struct ZoneVoice;

const ZONE_WORD: u16 = 471;
const HUNDRED: u16 = 49;
const ONES: [u16; 10] = [0, 22, 23, 24, 25, 26, 27, 28, 29, 30];
const TEENS: [u16; 10] = [31, 32, 33, 34, 35, 36, 37, 38, 39, 40];
const TENS: [u16; 10] = [0, 0, 41, 42, 43, 44, 45, 46, 47, 48];

impl ZoneVoice {
    /// The six voice words of zone 1..=208.
    pub fn words(zone: i32) -> Result<[u16; 6]> {
        if !(1..=208).contains(&zone) {
            return Err(ElkError::OutOfRange("zone".into()));
        }
        let mut w = vec![ZONE_WORD];
        let r = (zone % 100) as usize;
        let h = (zone / 100) as usize;
        if h == 1 || h == 2 {
            w.push(ONES[h]);
            if r < 10 {
                w.push(HUNDRED);
            }
        }
        if (10..=19).contains(&r) {
            w.push(TEENS[r - 10]);
        } else if r / 10 >= 2 {
            w.push(TENS[r / 10]);
        }
        if !(10..=20).contains(&r) && !r.is_multiple_of(10) {
            w.push(ONES[r % 10]);
        }
        let mut out = [0u16; 6];
        out[..w.len()].copy_from_slice(&w);
        Ok(out)
    }

    /// Writes the default voice words of `zone` into the record.
    pub fn apply(z: &mut ZoneRecord, zone: i32) -> Result<()> {
        let w = Self::words(zone)?;
        for (i, word) in w.iter().enumerate() {
            z.bytes[20 + 2 * i] = (word >> 8) as u8;
            z.bytes[21 + 2 * i] = (word & 0xFF) as u8;
        }
        Ok(())
    }
}

/// What an unused item looks like on a factory-default M1G (firmware 5.3.44). Used to reset panel records that an account does not contain.
pub struct DefaultRecords;

fn name(b: &mut [u8], at: usize, s: &str, pad: u8) {
    for x in &mut b[at..at + 16] {
        *x = pad;
    }
    b[at..at + s.len()].copy_from_slice(s.as_bytes());
}

impl DefaultRecords {
    /// Default zone record (32 bytes). Panics when `n` is outside 1..=208 (C#: `ArgumentOutOfRangeException`).
    pub fn zone(n: i32) -> Vec<u8> {
        let mut z = ZoneRecord::new();
        z.set_function(0);
        z.set_flag1(1);
        z.set_flag2(0);
        z.set_name(&format!("Zone {n:03}"));
        ZoneVoice::apply(&mut z, n).expect("zone must be 1..=208");
        z.bytes.to_vec()
    }
    /// Default user record (28 bytes).
    pub fn user(n: i32) -> Vec<u8> {
        let mut b = vec![0u8; 28];
        b[6] = 0xE2;
        b[8] = 1;
        name(&mut b, 9, &format!("USER {n:03}"), 0);
        b
    }
    /// Default area record (24 bytes).
    pub fn area(n: i32) -> Vec<u8> {
        let mut b = vec![0u8; 24];
        name(&mut b, 0, &format!("Area {n}"), b' ');
        b[16] = 60;
        b[17] = 30;
        b[18] = 60;
        b[19] = 30;
        b[20] = 0x61;
        b[21] = 0x50;
        b[22] = 15;
        b
    }
    /// Default keypad record (148 bytes).
    pub fn keypad() -> Vec<u8> {
        let mut b = vec![0u8; 148];
        b[4] = 0x33;
        b[5] = 0x60;
        b
    }
    /// Default telephone record (88 bytes).
    pub fn phone() -> Vec<u8> {
        let mut b = vec![0u8; 88];
        b[20] = 0xA8;
        b[22] = 8;
        for x in &mut b[23..71] {
            *x = b'0';
        }
        b
    }
    /// Default area report code record (20 bytes).
    pub fn area_rc() -> Vec<u8> {
        vec![0; 20]
    }
    /// Default user report code record (4 bytes).
    pub fn user_rc() -> Vec<u8> {
        vec![1, 0, 0, 1]
    }
    /// Default zone report code record (4 bytes).
    pub fn zone_rc() -> Vec<u8> {
        vec![0; 4]
    }
}
