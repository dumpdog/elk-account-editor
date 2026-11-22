//! Zone import rows: the default zone voice, the `ZoneSpec` row and the zone CSV (`Zone,Name,Definition,Type,Area,Attributes`).

use std::collections::{HashMap, HashSet};

use crate::error::{parse_int_loose, Error, Result};
use crate::records::{ZoneFlag1, ZoneFlag2, ZoneRecord};

/// Default zone voice description, as built by ElkRP: "Zone", then for 100-299 "One"/"Two" (plus "Hundred" if the remainder is
/// under 10), then the teens word or the tens word, then the ones digit when the remainder is under 10 or over 20.
pub struct ZoneVoice;

impl ZoneVoice {
    const ZONE: u16 = 471;
    const HUNDRED: u16 = 49;
    const ONES: [u16; 10] = [0, 22, 23, 24, 25, 26, 27, 28, 29, 30];
    const TEENS: [u16; 10] = [31, 32, 33, 34, 35, 36, 37, 38, 39, 40];
    const TENS: [u16; 10] = [0, 0, 41, 42, 43, 44, 45, 46, 47, 48];

    pub fn words(zone: i32) -> Result<[u16; 6]> {
        if !(1..=208).contains(&zone) {
            return Err(Error::OutOfRange(
                "Specified argument was out of the range of valid values. (Parameter 'zone')"
                    .into(),
            ));
        }
        let mut w: Vec<u16> = vec![Self::ZONE];
        let r = (zone % 100) as usize;
        let h = (zone / 100) as usize;
        if h == 1 || h == 2 {
            w.push(Self::ONES[h]);
            if r < 10 {
                w.push(Self::HUNDRED);
            }
        }
        if (10..=19).contains(&r) {
            w.push(Self::TEENS[r - 10]);
        } else if r / 10 >= 2 {
            w.push(Self::TENS[r / 10]);
        }
        if !(10..=20).contains(&r) && !r.is_multiple_of(10) {
            w.push(Self::ONES[r % 10]);
        }
        let mut out = [0u16; 6];
        for (i, v) in w.iter().enumerate().take(6) {
            out[i] = *v;
        }
        Ok(out)
    }

    pub fn apply(z: &mut ZoneRecord, zone: i32) -> Result<()> {
        let w = Self::words(zone)?;
        for (i, v) in w.iter().enumerate() {
            z.bytes[20 + 2 * i] = (v >> 8) as u8;
            z.bytes[21 + 2 * i] = (v & 0xFF) as u8;
        }
        Ok(())
    }
}

/// One row of the import file: zone, name, definition, type, area, attributes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneSpec {
    pub zone: i32,
    pub name: String,
    pub definition: i32,
    pub type_: i32,
    pub area: i32,
    /// `ZoneFlag1` bits.
    pub flags1: u8,
    pub swinger_shutdown: bool,
    pub silent_alarm: bool,
}

impl ZoneSpec {
    pub fn to_record(&self) -> Result<ZoneRecord> {
        let mut z = ZoneRecord::default();
        z.set_function(self.definition as u8);
        z.set_flag1(self.flags1);
        z.set_name(&self.name);
        let mut f2 = ZoneFlag2::with(0, Some(self.type_), Some(self.area));
        if self.swinger_shutdown {
            f2 |= ZoneFlag2::SWINGER_SHUTDOWN;
        }
        if self.silent_alarm {
            f2 |= ZoneFlag2::SILENT_ALARM;
        }
        z.set_flag2(f2);
        ZoneVoice::apply(&mut z, self.zone)?;
        Ok(z)
    }
}

/// CSV: Zone,Name,Definition,Type,Area,Attributes. Attributes are separated by `|` `;` or `,` inside quotes.
pub struct ZoneCsv;

impl ZoneCsv {
    pub const HEADER: &'static str = "Zone,Name,Definition,Type,Area,Attributes";

    fn flag1_names() -> HashMap<&'static str, u8> {
        HashMap::from([
            ("bypassable", ZoneFlag1::BYPASSABLE),
            ("periodictrip", ZoneFlag1::PERIODIC_TRIP),
            ("fastloopresponse", ZoneFlag1::FAST_RESPONSE),
            ("forcearmable", ZoneFlag1::FORCE_ARM),
            ("enablechime", ZoneFlag1::CHIME),
            ("incrosszonepool", ZoneFlag1::CROSS_ZONED),
            ("usedialerdelay", ZoneFlag1::USE_DIALER_DELAY),
            ("listenin", ZoneFlag1::LISTEN_IN),
        ])
    }

    /// `int.Parse` with .NET's FormatException / OverflowException texts.
    fn parse_int(s: &str) -> Result<i32> {
        let t = s.trim();
        let digits = t.strip_prefix(['+', '-']).unwrap_or(t);
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err(crate::error::bad_number(s));
        }
        t.parse::<i32>().map_err(|_| {
            Error::Format("Value was either too large or too small for an Int32.".into())
        })
    }

    pub fn parse(text: &str) -> (Vec<ZoneSpec>, Vec<String>) {
        let mut zones = Vec::new();
        let mut errors = Vec::new();
        let mut seen = HashSet::new();
        let names = Self::flag1_names();
        let lines: Vec<&str> = text.split('\n').map(|l| l.trim_end_matches('\r')).collect();
        for (i, line) in lines.iter().enumerate() {
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let f = split_csv(line);
            if i == 0 && parse_int_loose(&f[0]).is_none() {
                continue; // header
            }
            let row = (|| -> Result<ZoneSpec> {
                if f.len() < 5 {
                    return Err(Error::format(
                        "need Zone,Name,Definition,Type,Area[,Attributes]",
                    ));
                }
                let zone = Self::parse_int(&f[0])?;
                let def = Self::parse_int(&f[2])?;
                let ty = Self::parse_int(&f[3])?;
                let area = Self::parse_int(&f[4])?;
                if !(1..=208).contains(&zone) {
                    return Err(Error::format(format!("zone {zone} out of range 1-208")));
                }
                if !seen.insert(zone) {
                    return Err(Error::format(format!("zone {zone} listed twice")));
                }
                if f[1].encode_utf16().count() > 16
                    || f[1].chars().any(|c| (c as u32) > 126 || (c as u32) < 32)
                {
                    return Err(Error::format(
                        "name must be at most 16 printable ASCII characters",
                    ));
                }
                if !(0..=255).contains(&def) {
                    return Err(Error::format("definition must be 0-255"));
                }
                if !(0..=7).contains(&ty) {
                    return Err(Error::format("type must be 0-7"));
                }
                if !(1..=8).contains(&area) {
                    return Err(Error::format("area must be 1-8"));
                }
                let (mut flags, mut swinger, mut silent) = (0u8, false, false);
                let attrs = f.get(5).map(String::as_str).unwrap_or("");
                for a in attrs.split(['|', ';', ',']).filter(|a| !a.is_empty()) {
                    let key: String = a
                        .chars()
                        .filter(|c| c.is_alphabetic())
                        .flat_map(char::to_lowercase)
                        .collect();
                    if key == "swingershutdown" {
                        swinger = true;
                    } else if key == "silentalarm" {
                        silent = true;
                    } else if let Some(fl) = names.get(key.as_str()) {
                        flags |= fl;
                    } else {
                        return Err(Error::format(format!("unknown attribute '{}'", a.trim())));
                    }
                }
                Ok(ZoneSpec {
                    zone,
                    name: f[1].clone(),
                    definition: def,
                    type_: ty,
                    area,
                    flags1: flags,
                    swinger_shutdown: swinger,
                    silent_alarm: silent,
                })
            })();
            match row {
                Ok(z) => zones.push(z),
                Err(e) => errors.push(format!("line {}: {}", i + 1, e.message())),
            }
        }
        (zones, errors)
    }
}

fn split_csv(line: &str) -> Vec<String> {
    let mut f = Vec::new();
    let mut sb = String::new();
    let mut q = false;
    for c in line.chars() {
        if c == '"' {
            q = !q;
        } else if c == ',' && !q {
            f.push(sb.trim().to_string());
            sb.clear();
        } else {
            sb.push(c);
        }
    }
    f.push(sb.trim().to_string());
    f
}
