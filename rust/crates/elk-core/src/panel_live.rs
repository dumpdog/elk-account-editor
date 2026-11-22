//! Port of PanelLive.cs: the live status of "Current Status and Remote Control" (queries 6, 7, 8 and 16).

use crate::elk_session::ElkSession;
use crate::error::{ElkError, Result};

/// One area's live state (from the panel's status query 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AreaLive {
    /// Area number 1-8.
    pub area: i32,
    /// Any of the "ready" bits set.
    pub ready: bool,
    /// Ready to force arm.
    pub ready_to_force_arm: bool,
    /// Armed level 0-7.
    pub armed: i32,
    /// Name of the armed level.
    pub armed_name: String,
    /// Alarm bits both set.
    pub alarm: bool,
}

/// One zone's live state (status queries 8 and 16).
#[derive(Debug, Clone, PartialEq)]
pub struct ZoneLive {
    /// Zone number 1-208.
    pub zone: i32,
    /// `Normal`, `Trouble` or `Violated`.
    pub state: String,
    /// Bypassed.
    pub bypassed: bool,
    /// Force armed.
    pub force_armed: bool,
    /// `N/A`, `Open (+)`, `EOLR (=)` or `Short (-)`.
    pub physical: String,
    /// Zone voltage.
    pub volts: f64,
}

/// Areas, zones and system alarm/trouble names read in one go.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveSnapshot {
    /// Areas.
    pub areas: Vec<AreaLive>,
    /// Zones.
    pub zones: Vec<ZoneLive>,
    /// System alarms and troubles.
    pub system: Vec<String>,
}

/// The live status ElkRP shows in "Current Status and Remote Control". All four queries are read only: `7F 00 00 n` with n = 6 (areas),
/// 7 (system alarms and troubles), 8 (zones) and 16 (zone voltages). Decoding follows ElkRP's own RemoteControl form.
pub struct PanelLive;

/// Names of the armed levels.
pub const ARMED_NAMES: [&str; 7] = ["Disarmed", "Armed Away", "Armed Stay", "Armed Stay Instant", "Armed Night", "Armed Night Instant", "Armed Vacation"];
const LOGICAL_NAMES: [&str; 4] = ["Normal", "Trouble", "Violated", "Violated"];
const PHYSICAL_NAMES: [&str; 4] = ["N/A", "Open (+)", "EOLR (=)", "Short (-)"];

/// Names of the system alarm and trouble bytes (reply bytes 4..55).
pub const SYSTEM_NAMES: [&str; 52] = [
    "",
    "",
    "Fire Alarm",
    "Medical Trouble/Alarm",
    "Police Trouble/Alarm",
    "Burglar Trouble/Alarm",
    "Aux 1 Trouble/Alarm",
    "Aux 2 Trouble/Alarm",
    "",
    "",
    "Carbon Monoxide Trouble/Alarm",
    "Emergency Trouble/Alarm",
    "Freeze Trouble/Alarm",
    "Gas Trouble/Alarm",
    "Heat Trouble/Alarm",
    "Water Trouble/Alarm",
    "Fire Supervisory Alarm",
    "Fire 'Verified' Alarm",
    "A.C. Failure",
    "Box Tamper",
    "Dialer Communication Failure",
    "EEPROM Memory Error",
    "Control Low Battery",
    "Transmitter Low Battery",
    "Control Over Current",
    "Telephone Fault",
    "",
    "Output 2 Trouble",
    "Keypad Trouble",
    "Input Expander Trouble",
    "Output Expander Trouble",
    "Audio Network Controller Trouble",
    "",
    "",
    "",
    "Flash Memory Error",
    "Security (Day) Alert",
    "Serial Port Expander Trouble",
    "Lost Transmitter Trouble",
    "Smoke Detector Needs Cleaning",
    "Ethernet Expander Trouble",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    "Fire Trouble",
];

impl PanelLive {
    fn query(s: &mut ElkSession, n: u8, min_length: usize) -> Result<Vec<u8>> {
        s.send(&[0x7F, 0, 0, n])?;
        let r = s.receive(0x7F)?;
        if r.len() < min_length || r[3] != n {
            return Err(ElkError::InvalidData(format!("unexpected reply to status query {n}: {} bytes", r.len())));
        }
        Ok(r)
    }

    /// Decodes the reply to query 6.
    pub fn decode_areas(r: &[u8]) -> Vec<AreaLive> {
        let mut list = Vec::new();
        let mut a = 1usize;
        while a <= 8 && 8 * a < r.len() {
            let b = r[8 * a] as i32;
            let ready = b & 0x1C;
            let armed = (b >> 5) & 7;
            list.push(AreaLive {
                area: a as i32,
                ready: ready != 0,
                ready_to_force_arm: ready == 8,
                armed,
                armed_name: if (armed as usize) < ARMED_NAMES.len() { ARMED_NAMES[armed as usize].to_string() } else { format!("Armed ({armed})") },
                alarm: (b & 3) == 3,
            });
            a += 1;
        }
        list
    }

    /// Decodes the reply to query 7.
    pub fn decode_system(r: &[u8]) -> Vec<String> {
        let mut list = Vec::new();
        let mut i = 4usize;
        while i <= 55 && i < r.len() {
            if r[i] != 0 {
                list.push(if i - 4 < SYSTEM_NAMES.len() && !SYSTEM_NAMES[i - 4].is_empty() { SYSTEM_NAMES[i - 4].to_string() } else { format!("Status item {}", i - 4) });
            }
            i += 1;
        }
        list
    }

    /// Decodes the replies to queries 8 (`status`) and 16 (`volts`).
    pub fn decode_zones(status: &[u8], volts: Option<&[u8]>) -> Vec<ZoneLive> {
        let mut list = Vec::new();
        let mut z = 1usize;
        while z <= 208 && z + 3 < status.len() {
            let b = status[z + 3] as usize;
            list.push(ZoneLive {
                zone: z as i32,
                state: LOGICAL_NAMES[(b >> 2) & 3].to_string(),
                bypassed: (b & 0x20) != 0,
                force_armed: (b & 0x10) != 0,
                physical: PHYSICAL_NAMES[b & 3].to_string(),
                volts: match volts {
                    Some(v) if z + 3 < v.len() => v[z + 3] as f64 / 10.0,
                    _ => 0.0,
                },
            });
            z += 1;
        }
        list
    }

    /// Reads the current status of the whole panel (four round trips).
    pub fn read(s: &mut ElkSession) -> Result<LiveSnapshot> {
        let areas = Self::query(s, 6, 68)?;
        let system = Self::query(s, 7, 56)?;
        let zones = Self::query(s, 8, 212)?;
        let volts = Self::query(s, 16, 212)?;
        Ok(LiveSnapshot { areas: Self::decode_areas(&areas), zones: Self::decode_zones(&zones, Some(&volts)), system: Self::decode_system(&system) })
    }
}
