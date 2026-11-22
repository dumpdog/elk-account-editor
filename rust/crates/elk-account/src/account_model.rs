//! Friendly, JSON-serialisable view of the sections of an [`AccountFile`]. Fields that are decoded are editable by name;
//! every other byte of a record is left exactly as ElkRP wrote it. JSON names are camelCase and the field order is the
//! C# property order, so the output equals what System.Text.Json (web defaults) produced.

use serde::{Deserialize, Serialize};

use crate::account_file::AccountFile;
use crate::automation::{AutomationCodec, AutomationDto};
use crate::error::{Error, Result};
use crate::records::{ZoneFlag2, ZoneRecord};
use crate::zone_import::ZoneVoice;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct FlagDef {
    pub key: &'static str,
    pub label: &'static str,
    pub byte: usize,
    pub bit: u8,
}

const fn fd(key: &'static str, label: &'static str, byte: usize, bit: u8) -> FlagDef {
    FlagDef {
        key,
        label,
        byte,
        bit,
    }
}

/// Labels and flag tables shared with the page (`/api/meta`).
pub struct ModelMeta;

impl ModelMeta {
    // Communicator report-code labels, in file order (Report_Codes enums). Area RC bytes 2-18, system RC bytes 4-21.
    pub const AREA_RC_LABELS: [&'static str; 17] = [
        "Abort",
        "Cancel",
        "Auto close",
        "Early close",
        "Closing extended",
        "Fail to close",
        "Late close",
        "Key switch close",
        "Duress",
        "Exception open",
        "Early open",
        "Fail to open",
        "Late open",
        "Key switch open",
        "Keypad lockout",
        "Exit error",
        "Fail to communicate",
    ];
    pub const SYSTEM_RC_LABELS: [&'static str; 18] = [
        "AC fail",
        "AC restore",
        "Auto test",
        "Aux over-current",
        "Aux over-current restore",
        "Expansion module trouble",
        "Expansion module restore",
        "Installer on premises",
        "Installer off premises",
        "Log 80% full",
        "Low battery",
        "Low battery restore",
        "Memory error",
        "Remote programming end",
        "Phone line fault",
        "Phone line restore",
        "Output 2 fault",
        "Output 2 restore",
    ];
    pub const ZONE_RC_LABELS: [&'static str; 4] = ["Alarm", "Restoral", "Bypass", "Trouble"];
    pub const USER_RC_LABELS: [&'static str; 2] = ["Close", "Open"];

    pub const ZONE_FLAGS: [FlagDef; 10] = [
        fd("bypassable", "Bypassable", 1, 0),
        fd("periodicTrip", "Periodic trip", 1, 1),
        fd("fastLoop", "Fast loop response", 1, 2),
        fd("forceArm", "Force armable", 1, 3),
        fd("chime", "Enable chime", 1, 4),
        fd("crossZone", "In cross-zone pool", 1, 5),
        fd("dialerDelay", "Use dialer delay", 1, 6),
        fd("listenIn", "Listen in", 1, 7),
        fd("swinger", "Swinger shutdown", 2, 3),
        fd("silent", "Silent alarm", 2, 7),
    ];
    pub const USER_FLAGS: [FlagDef; 16] = [
        fd("arm", "Arm", 6, 7),
        fd("disarm", "Disarm", 6, 6),
        fd("bypass", "Bypass", 6, 5),
        fd("access", "Access", 6, 4),
        fd("master", "Master", 6, 2),
        fd("menus", "User menus 1-5 allowed", 6, 1),
        fd("duress", "Duress", 7, 0),
        fd("temporary", "Temporary code", 25, 7),
        fd("area1", "Area 1", 8, 0),
        fd("area2", "Area 2", 8, 1),
        fd("area3", "Area 3", 8, 2),
        fd("area4", "Area 4", 8, 3),
        fd("area5", "Area 5", 8, 4),
        fd("area6", "Area 6", 8, 5),
        fd("area7", "Area 7", 8, 6),
        fd("area8", "Area 8", 8, 7),
    ];
    pub const AREA_FLAGS: [FlagDef; 10] = [
        fd("quickArm1", "Single keypress quick-arm", 20, 0),
        fd("restartExit", "Restart exit timers", 20, 1),
        fd("ringback", "Enable closing ringback", 20, 2),
        fd(
            "autoStay",
            "Auto Stay mode after exit time if no violation",
            20,
            4,
        ),
        fd("stayKeyChange", "Allow STAY key change if armed", 20, 6),
        fd("quickArm2", "Double keypress quick-arm", 21, 7),
        fd("stay", "STAY scroll: Stay", 21, 6),
        fd("stayInstant", "STAY scroll: Stay Instant", 21, 5),
        fd("night", "STAY scroll: Night", 21, 4),
        fd("nightInstant", "STAY scroll: Night Instant", 21, 3),
    ];
    pub const PHONE_FLAGS: [FlagDef; 5] = [
        fd(
            "areaEvents",
            "Area events, alarms, restorals, unbypasses",
            20,
            7,
        ),
        fd("zoneBypass", "Zone bypasses", 20, 6),
        fd("zoneTrouble", "Zone troubles", 20, 5),
        fd("userEvents", "User events (opens/closings)", 20, 4),
        fd("globalEvents", "Global system events", 20, 3),
    ];

    pub const ZONE_DEFINITIONS: [&'static str; 37] = [
        "Disabled",
        "Burglar Entry/Exit 1",
        "Burglar Entry/Exit 2",
        "Burglar Perimeter Instant",
        "Burglar Interior",
        "Burglar Interior Follower",
        "Burglar Interior Night",
        "Burglar Interior Night Delay",
        "Burglar 24 Hour",
        "Burglar Box Tamper",
        "Fire Alarm",
        "Fire Verified",
        "Fire Supervisory",
        "Aux1 24Hr. Alarm",
        "Aux2 24Hr. Alarm",
        "Keyfob",
        "Non-alarm",
        "Carbon Monoxide",
        "Emergency Alarm",
        "Freeze Alarm",
        "Gas Alarm",
        "Heat Alarm",
        "Medical Alarm",
        "Police Alarm",
        "Police No Indication",
        "Water Alarm",
        "Key Momentary Arm/Disarm",
        "Key Momentary Arm Away",
        "Key Momentary Arm Stay",
        "Key Momentary Disarm",
        "Key On/Off",
        "Mute Audibles",
        "Power Supervisory",
        "Temperature",
        "(34)",
        "Area Arm",
        "Intercom Key",
    ];
    pub const ZONE_TYPES: [&'static str; 7] = [
        "EOL Hardwire / Wireless",
        "Normally Closed",
        "Normally Open",
        "EOL Supervised on Short",
        "EOL Supervised on Open",
        "Fire / 4-Wire Smoke",
        "2-Wire Smoke on Zone 16",
    ];
}

fn one() -> i32 {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ZoneDto {
    pub n: i32,
    pub name: String,
    pub definition: i32,
    #[serde(rename = "type")]
    pub type_: i32,
    pub area: i32,
    pub flags: Vec<String>,
    pub default_voice: bool,
}

impl Default for ZoneDto {
    fn default() -> Self {
        ZoneDto {
            n: 0,
            name: String::new(),
            definition: 0,
            type_: 0,
            area: one(),
            flags: Vec::new(),
            default_voice: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UserDto {
    pub n: i32,
    pub name: String,
    pub code: String,
    pub flags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AreaDto {
    pub n: i32,
    pub name: String,
    pub exit1: i32,
    pub entrance1: i32,
    pub exit2: i32,
    pub entrance2: i32,
    pub arm_window: i32,
    pub flags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FKeyDto {
    pub definition: i32,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct KeypadDto {
    pub n: i32,
    pub name: String,
    pub area: i32,
    pub f_keys: Vec<FKeyDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PhoneDto {
    pub n: i32,
    pub name: String,
    pub number: String,
    pub format: i32,
    pub attempts: i32,
    /// `string[8]` in C#: entries may be null.
    pub account_codes: Vec<Option<String>>,
    pub flags: Vec<String>,
}

impl Default for PhoneDto {
    fn default() -> Self {
        PhoneDto {
            n: 0,
            name: String::new(),
            number: String::new(),
            format: 0,
            attempts: 0,
            account_codes: vec![None; 8],
            flags: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AreaRcDto {
    pub n: i32,
    pub abort_time: i32,
    pub cancel_time: i32,
    pub codes: Vec<i32>,
}

impl Default for AreaRcDto {
    fn default() -> Self {
        AreaRcDto {
            n: 0,
            abort_time: 0,
            cancel_time: 0,
            codes: vec![0; 17],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UserRcDto {
    pub n: i32,
    pub close: i32,
    pub open: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ZoneRcDto {
    pub n: i32,
    pub codes: Vec<i32>,
}

impl Default for ZoneRcDto {
    fn default() -> Self {
        ZoneRcDto {
            n: 0,
            codes: vec![0; 4],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct KeypadRcDto {
    pub n: i32,
    pub codes: Vec<i32>,
}

impl Default for KeypadRcDto {
    fn default() -> Self {
        KeypadRcDto {
            n: 0,
            codes: vec![0; 6],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SystemRcDto {
    pub ac_fail_delay: i32,
    pub test_hours: i32,
    pub test_minutes: i32,
    pub test_days: i32,
    pub codes: Vec<i32>,
}

impl Default for SystemRcDto {
    fn default() -> Self {
        SystemRcDto {
            ac_fail_delay: 0,
            test_hours: 0,
            test_minutes: 0,
            test_days: 0,
            codes: vec![0; 18],
        }
    }
}

/// Communicator report codes. Every code is one byte (ElkRP shows it as two hex digits); 00 = not reported.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CommDto {
    pub areas: Vec<AreaRcDto>,
    pub users: Vec<UserRcDto>,
    pub zones: Vec<ZoneRcDto>,
    pub system: SystemRcDto,
    pub keypads: Vec<KeypadRcDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AccountModel {
    pub comm: Option<CommDto>,
    pub account: String,
    pub panel_type: String,
    pub software: String,
    pub code_length: i32,
    /// The Globals values (48, or 36 for software up to 4.3) as the file has them, or None when the file has no globals.
    pub globals: Option<Vec<i32>>,
    /// The wireless receiver blocks, 260 values each (3 for software 5.x and later), or None.
    pub wireless: Option<Vec<Vec<i32>>>,
    pub zones: Vec<ZoneDto>,
    /// 13 flags: group g = zones 16(g-1)+1 .. 16g
    pub zone_groups: Vec<i32>,
    pub users: Vec<UserDto>,
    pub areas: Vec<AreaDto>,
    pub keypads: Vec<KeypadDto>,
    pub phones: Vec<PhoneDto>,
    pub has_phones: bool,
    /// Tasks, counters, custom values, outputs, lights, voice messages, rules and texts, or None when the file has none.
    pub automation: Option<AutomationDto>,
    /// When true, `apply_to` also removes records that are in the file but not in this model.
    pub prune: bool,
    /// The Account Details page (name, address, contacts, notes, versions, network adapter), or None when the file head has an unknown layout.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<crate::details::DetailsDto>,
}

impl Default for AccountModel {
    fn default() -> Self {
        AccountModel {
            comm: None,
            account: String::new(),
            panel_type: String::new(),
            software: String::new(),
            code_length: 4,
            globals: None,
            wireless: None,
            zones: Vec::new(),
            zone_groups: Vec::new(),
            users: Vec::new(),
            areas: Vec::new(),
            keypads: Vec::new(),
            phones: Vec::new(),
            has_phones: false,
            automation: None,
            prune: false,
            details: None,
        }
    }
}

// ---- helpers --------------------------------------------------------------------------------------------------------

/// Latin-1 text of a field with trailing NUL and space removed.
fn text(b: &[u8], at: usize, len: usize) -> String {
    let s: String = b[at..at + len].iter().map(|&c| c as char).collect();
    s.trim_end_matches(['\0', ' ']).to_string()
}

/// Only rewrites a text field when its visible text changed, so ElkRP's own padding (NUL or space) survives untouched fields.
fn put_text(b: &mut [u8], at: usize, len: usize, s: &str) {
    let units: Vec<u16> = s.encode_utf16().collect();
    let cut: &[u16] = if units.len() > len {
        &units[..len]
    } else {
        &units
    };
    let mut want = cut.to_vec();
    while want.last() == Some(&32) {
        want.pop();
    }
    let mut cur: Vec<u16> = b[at..at + len].iter().map(|&c| c as u16).collect();
    while matches!(cur.last(), Some(&0) | Some(&32)) {
        cur.pop();
    }
    if cur == want {
        return;
    }
    b[at..at + len].iter_mut().for_each(|x| *x = 0);
    let encoded: Vec<u8> = String::from_utf16_lossy(cut)
        .chars()
        .map(|c| {
            if (c as u32) < 256 {
                c as u32 as u8
            } else {
                b'?'
            }
        })
        .collect();
    b[at..at + encoded.len()].copy_from_slice(&encoded);
}

fn get_flags(b: &[u8], defs: &[FlagDef]) -> Vec<String> {
    defs.iter()
        .filter(|d| b[d.byte] & (1 << d.bit) != 0)
        .map(|d| d.key.to_string())
        .collect()
}

fn set_flags(b: &mut [u8], defs: &[FlagDef], on: &[String]) {
    for d in defs {
        if on.iter().any(|k| k == d.key) {
            b[d.byte] |= 1 << d.bit;
        } else {
            b[d.byte] &= !(1 << d.bit);
        }
    }
}

fn clamp(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

fn rc(v: i32, what: &str) -> Result<u8> {
    if (0..=255).contains(&v) {
        Ok(v as u8)
    } else {
        Err(Error::format(format!("{what}: report code must be 00-FF")))
    }
}

impl AccountModel {
    pub fn decode_code(b: &[u8], len: usize) -> String {
        (0..len)
            .rev()
            .map(|i| {
                if b[i] <= 9 {
                    (b'0' + b[i]) as char
                } else {
                    '?'
                }
            })
            .collect()
    }

    pub fn encode_code(b: &mut [u8], code: &str, len: usize) -> Result<()> {
        let digits: Vec<u8> = code
            .chars()
            .filter(|c| c.is_ascii_digit())
            .map(|c| c as u8)
            .collect();
        if digits.len() != len {
            return Err(Error::format(format!("user code must be {len} digits")));
        }
        for x in b.iter_mut().take(6) {
            *x = 0;
        }
        for i in 0..len {
            b[i] = digits[len - 1 - i] - b'0';
        }
        Ok(())
    }

    // ---- decode ---------------------------------------------------------------------------------------------------
    #[allow(clippy::should_implement_trait)]
    pub fn from(f: &AccountFile) -> Result<AccountModel> {
        let mut m = AccountModel {
            account: f.account_id().to_string(),
            panel_type: f.panel_type().to_string(),
            software: f.software_version().to_string(),
            has_phones: f.has_phone_section(),
            ..Default::default()
        };
        m.zone_groups = f
            .zones
            .trailer
            .iter()
            .map(|s| crate::error::parse_int_loose(s).ok_or_else(|| crate::error::bad_number(s)))
            .collect::<Result<Vec<i32>>>()?;
        // Code length is a Globals option (byte 0 bit 2 = 6 digits); fall back to a guess from the users if the globals were not found.
        m.globals = f.globals()?.map(|g| g.iter().map(|&b| b as i32).collect());
        m.wireless = f.wireless()?.map(|w| {
            w.iter()
                .map(|blk| blk.iter().map(|&b| b as i32).collect())
                .collect()
        });
        m.code_length = f.code_length().unwrap_or_else(|| {
            if f.users
                .entries
                .iter()
                .any(|e| e.bytes[4] != 0 || e.bytes[5] != 0)
            {
                6
            } else {
                4
            }
        });
        for e in &f.zones.entries {
            let z = ZoneRecord::from_bytes(&e.bytes);
            m.zones.push(ZoneDto {
                n: e.number,
                name: z.name(),
                definition: z.function() as i32,
                type_: ZoneFlag2::type_of(z.flag2()),
                area: ZoneFlag2::area_of(z.flag2()),
                flags: get_flags(&e.bytes, &ModelMeta::ZONE_FLAGS),
                default_voice: false,
            });
        }
        for e in &f.users.entries {
            m.users.push(UserDto {
                n: e.number,
                name: text(&e.bytes, 9, 16),
                code: Self::decode_code(&e.bytes, m.code_length as usize),
                flags: get_flags(&e.bytes, &ModelMeta::USER_FLAGS),
            });
        }
        for e in &f.areas.entries {
            let b = &e.bytes;
            m.areas.push(AreaDto {
                n: e.number,
                name: text(b, 0, 16),
                exit1: b[16] as i32,
                entrance1: b[17] as i32,
                exit2: b[18] as i32,
                entrance2: b[19] as i32,
                arm_window: b[22] as i32,
                flags: get_flags(b, &ModelMeta::AREA_FLAGS),
            });
        }
        for e in &f.keypads.entries {
            let b = &e.bytes;
            let mut k = KeypadDto {
                n: e.number,
                name: text(b, 6, 16),
                area: b[0] as i32,
                f_keys: Vec::new(),
            };
            for i in 0..6 {
                k.f_keys.push(FKeyDto {
                    definition: ((b[28 + 2 * i] as i32) << 8) | b[29 + 2 * i] as i32,
                    name: text(b, 52 + 16 * i, 16),
                });
            }
            m.keypads.push(k);
        }
        for e in &f.phones.entries {
            let b = &e.bytes;
            let mut p = PhoneDto {
                n: e.number,
                number: text(b, 0, 20),
                name: text(b, 71, 16),
                format: (b[21] >> 4) as i32,
                attempts: b[22] as i32,
                flags: get_flags(b, &ModelMeta::PHONE_FLAGS),
                ..Default::default()
            };
            for (a, slot) in p.account_codes.iter_mut().enumerate() {
                *slot = Some(text(b, 23 + 6 * a, 6));
            }
            m.phones.push(p);
        }
        if f.has_comm() {
            let mut c = CommDto::default();
            for e in &f.area_rcs.entries {
                c.areas.push(AreaRcDto {
                    n: e.number,
                    abort_time: e.bytes[0] as i32,
                    cancel_time: e.bytes[1] as i32,
                    codes: e.bytes.iter().skip(2).take(17).map(|&b| b as i32).collect(),
                });
            }
            for e in &f.user_rcs.entries {
                c.users.push(UserRcDto {
                    n: e.number,
                    close: e.bytes[0] as i32,
                    open: e.bytes[1] as i32,
                });
            }
            for e in &f.zone_rcs.entries {
                c.zones.push(ZoneRcDto {
                    n: e.number,
                    codes: e.bytes.iter().map(|&b| b as i32).collect(),
                });
            }
            let sr = &f.system_rc;
            c.system = SystemRcDto {
                ac_fail_delay: sr[0] as i32,
                test_hours: sr[1] as i32,
                test_minutes: sr[2] as i32,
                test_days: sr[3] as i32,
                codes: sr.iter().skip(4).take(18).map(|&b| b as i32).collect(),
            };
            for k in 0..16 {
                c.keypads.push(KeypadRcDto {
                    n: k as i32 + 1,
                    codes: f
                        .keypad_rc
                        .iter()
                        .skip(6 * k)
                        .take(6)
                        .map(|&b| b as i32)
                        .collect(),
                });
            }
            m.comm = Some(c);
        }
        if let Some(a) = &f.automation {
            m.automation = Some(AutomationCodec::from(a));
        }
        m.details = f.details();
        Ok(m)
    }

    /// Elk rule (installation manual): the first wireless zone is always zone 17 and no wireless zone may exist beyond zone 160,
    /// so group 1 (zones 1-16) and groups past zone 160 cannot be wireless. Gaps are allowed.
    pub fn validate_wireless_groups(g: &[i32]) -> Result<()> {
        if g.is_empty() {
            return Ok(());
        }
        if g[0] != 0 {
            return Err(Error::format(
                "wireless zone groups: group 1 (zones 1-16) cannot be wireless",
            ));
        }
        for v in g.iter().skip(10) {
            if *v != 0 {
                return Err(Error::format(
                    "wireless zone groups: no wireless zone may be above zone 160",
                ));
            }
        }
        Ok(())
    }

    // ---- apply ----------------------------------------------------------------------------------------------------
    /// Writes the model into the file's sections. Records that already exist keep every byte this tool does not edit.
    pub fn apply_to(&self, f: &mut AccountFile) -> Result<()> {
        if let (Some(wl), Some(w0)) = (&self.wireless, f.wireless()?) {
            if wl.len() != w0.len()
                || wl
                    .iter()
                    .any(|b| b.len() != 260 || b.iter().any(|v| !(0..=255).contains(v)))
            {
                return Err(Error::format(format!(
                    "wireless: need {} blocks of 260 values (0-255)",
                    w0.len()
                )));
            }
            for i in 0..w0.len() {
                let nb: Vec<u8> = wl[i].iter().map(|&v| v as u8).collect();
                if nb != w0[i] {
                    f.set_wireless(i, &nb)?;
                }
            }
        }
        if let (Some(gl), Some(g0)) = (&self.globals, f.globals()?) {
            if gl.len() != g0.len() || gl.iter().any(|v| !(0..=255).contains(v)) {
                return Err(Error::format(format!(
                    "globals: need {} values of 0-255",
                    g0.len()
                )));
            }
            let gb: Vec<u8> = gl.iter().map(|&v| v as u8).collect();
            if gb != g0 {
                f.set_globals(&gb)?;
            }
        }
        for z in &self.zones {
            let is_new = f.zones.get(z.n).is_none();
            let e = f.zones.get_or_add(z.n)?;
            if z.name.encode_utf16().count() > 16
                || z.name.chars().any(|c| (c as u32) < 32 || (c as u32) > 126)
            {
                return Err(Error::format(format!(
                    "zone {}: name must be at most 16 printable ASCII characters",
                    z.n
                )));
            }
            if !(0..=7).contains(&z.type_) {
                return Err(Error::format(format!("zone {}: type must be 0-7", z.n)));
            }
            if !(1..=8).contains(&z.area) {
                return Err(Error::format(format!("zone {}: area must be 1-8", z.n)));
            }
            let mut rec = ZoneRecord::from_bytes(&e.bytes);
            if rec.name() != z.name {
                rec.set_name(&z.name);
            }
            rec.set_function(clamp(z.definition));
            set_flags(&mut rec.bytes, &ModelMeta::ZONE_FLAGS, &z.flags);
            let f2 = ZoneFlag2::with(rec.flag2(), Some(z.type_), Some(z.area));
            rec.set_flag2(f2);
            if is_new || z.default_voice {
                ZoneVoice::apply(&mut rec, z.n)?;
            }
            e.bytes[..32].copy_from_slice(&rec.bytes);
        }
        Self::validate_wireless_groups(&self.zone_groups)?;
        if self.zone_groups.len() == f.zones.trailer.len() && !self.zone_groups.is_empty() {
            f.zones.trailer = self.zone_groups.iter().map(|v| v.to_string()).collect();
        }

        let code_len = self.code_length;
        for u in &self.users {
            let e = f.users.get_or_add(u.n)?;
            put_text(&mut e.bytes, 9, 16, &u.name);
            if !u.code.chars().all(char::is_whitespace)
                && u.code != Self::decode_code(&e.bytes, code_len.max(0) as usize)
            {
                Self::encode_code(&mut e.bytes, &u.code, code_len.max(0) as usize)?;
            }
            set_flags(&mut e.bytes, &ModelMeta::USER_FLAGS, &u.flags);
        }
        for a in &self.areas {
            let e = f.areas.get_or_add(a.n)?;
            put_text(&mut e.bytes, 0, 16, &a.name);
            e.bytes[16] = clamp(a.exit1);
            e.bytes[17] = clamp(a.entrance1);
            e.bytes[18] = clamp(a.exit2);
            e.bytes[19] = clamp(a.entrance2);
            e.bytes[22] = clamp(a.arm_window);
            set_flags(&mut e.bytes, &ModelMeta::AREA_FLAGS, &a.flags);
        }
        for k in &self.keypads {
            let e = f.keypads.get_or_add(k.n)?;
            e.bytes[0] = clamp(k.area);
            put_text(&mut e.bytes, 6, 16, &k.name);
            for i in 0..6 {
                let Some(fk) = k.f_keys.get(i) else { break };
                e.bytes[28 + 2 * i] = (fk.definition >> 8) as u8;
                e.bytes[29 + 2 * i] = fk.definition as u8;
                put_text(&mut e.bytes, 52 + 16 * i, 16, &fk.name);
            }
        }
        for p in &self.phones {
            let e = f.phones.get_or_add(p.n)?;
            put_text(&mut e.bytes, 0, 20, &p.number);
            put_text(&mut e.bytes, 71, 16, &p.name);
            if !(0..=7).contains(&p.format) {
                return Err(Error::format(format!(
                    "telephone {}: reporting format must be 0-7",
                    p.n
                )));
            }
            e.bytes[21] = (e.bytes[21] & 0x0F) | ((p.format << 4) as u8);
            e.bytes[22] = clamp(p.attempts);
            if p.format == 6 && e.bytes[22] > 1 {
                e.bytes[22] = 1; // ElkRP: IP reporting allows a single attempt
            }
            for a in 0..8 {
                let code = p
                    .account_codes
                    .get(a)
                    .and_then(|c| c.as_deref())
                    .unwrap_or("000000");
                put_text(&mut e.bytes, 23 + 6 * a, 6, code);
            }
            set_flags(&mut e.bytes, &ModelMeta::PHONE_FLAGS, &p.flags);
        }
        if let Some(comm) = &self.comm {
            if !f.has_comm() {
                return Err(Error::format(
                    "this file has no communicator (report code) section",
                ));
            }
            for a in &comm.areas {
                let e = f.area_rcs.get_or_add(a.n)?;
                let w = format!("area {} report codes", a.n);
                e.bytes[0] = rc(a.abort_time, &w)?;
                e.bytes[1] = rc(a.cancel_time, &w)?;
                for i in 0..17 {
                    let Some(c) = a.codes.get(i) else { break };
                    e.bytes[2 + i] = rc(*c, &w)?;
                }
            }
            for u in &comm.users {
                let e = f.user_rcs.get_or_add(u.n)?;
                let w = format!("user {} report codes", u.n);
                e.bytes[0] = rc(u.close, &w)?;
                e.bytes[1] = rc(u.open, &w)?;
            }
            for z in &comm.zones {
                let e = f.zone_rcs.get_or_add(z.n)?;
                for (i, c) in z.codes.iter().enumerate().take(4) {
                    e.bytes[i] = rc(*c, &format!("zone {} report codes", z.n))?;
                }
            }
            let s = &comm.system;
            let sw = "system report codes";
            f.system_rc[0] = rc(s.ac_fail_delay, sw)?;
            f.system_rc[1] = rc(s.test_hours, sw)?;
            f.system_rc[2] = rc(s.test_minutes, sw)?;
            f.system_rc[3] = rc(s.test_days, sw)?;
            for i in 0..18 {
                let Some(c) = s.codes.get(i) else { break };
                f.system_rc[4 + i] = rc(*c, sw)?;
            }
            for k in &comm.keypads {
                if (1..=16).contains(&k.n) {
                    for i in 0..6 {
                        let Some(c) = k.codes.get(i) else { break };
                        f.keypad_rc[6 * (k.n as usize - 1) + i] =
                            rc(*c, &format!("keypad {} report codes", k.n))?;
                    }
                }
            }
        }
        if let Some(auto) = &self.automation {
            let Some(block) = f.automation.as_mut() else {
                return Err(Error::format(
                    "this file has no automation section (outputs, tasks, rules ...)",
                ));
            };
            AutomationCodec::apply(auto, block)?;
        }
        if let Some(d) = &self.details {
            f.set_details(d)?;
        }
        if self.prune {
            self.prune_missing(f)?;
        }
        Ok(())
    }

    fn prune_missing(&self, f: &mut AccountFile) -> Result<()> {
        if self.areas.is_empty() {
            return Err(Error::format("an account needs at least one area"));
        }
        if self.keypads.is_empty() {
            return Err(Error::format("an account needs at least one keypad"));
        }
        if !self.users.iter().any(|u| u.n == 1) {
            return Err(Error::format("user 1 (the master code) cannot be deleted"));
        }
        let removed_areas: Vec<i32> = f
            .areas
            .entries
            .iter()
            .map(|e| e.number)
            .filter(|n| self.areas.iter().all(|a| a.n != *n))
            .collect();
        f.zones
            .entries
            .retain(|e| self.zones.iter().any(|z| z.n == e.number));
        f.users
            .entries
            .retain(|e| self.users.iter().any(|u| u.n == e.number));
        f.areas
            .entries
            .retain(|e| self.areas.iter().any(|a| a.n == e.number));
        f.keypads
            .entries
            .retain(|e| self.keypads.iter().any(|k| k.n == e.number));
        f.phones
            .entries
            .retain(|e| self.phones.iter().any(|p| p.n == e.number));
        if let Some(comm) = &self.comm {
            f.area_rcs
                .entries
                .retain(|e| comm.areas.iter().any(|a| a.n == e.number));
            f.user_rcs
                .entries
                .retain(|e| comm.users.iter().any(|u| u.n == e.number));
            f.zone_rcs
                .entries
                .retain(|e| comm.zones.iter().any(|z| z.n == e.number));
            for k in 1..=16 {
                if self.keypads.iter().all(|x| x.n != k) {
                    // a deleted keypad reports nothing
                    f.keypad_rc[6 * (k as usize - 1)..6 * (k as usize - 1) + 6].fill(0);
                }
            }
        }
        // deleting an area that zones or keypads still use would leave them pointing at nothing
        for z in &self.zones {
            if removed_areas.contains(&z.area) {
                return Err(Error::format(format!(
                    "zone {} is in area {}, which is deleted",
                    z.n, z.area
                )));
            }
        }
        for k in &self.keypads {
            if removed_areas.contains(&k.area) {
                return Err(Error::format(format!(
                    "keypad {} is in area {}, which is deleted",
                    k.n, k.area
                )));
            }
        }
        Ok(())
    }
}

impl AccountModel {
    /// The JSON the web page consumes (System.Text.Json web defaults: camelCase, nulls written, compact).
    pub fn to_json(&self) -> String {
        crate::json::to_string(self)
    }

    /// Parses a request body like `/api/save` receives.
    pub fn from_json(s: &str) -> Result<AccountModel> {
        serde_json::from_str(s).map_err(|e| Error::format(e.to_string()))
    }
}
