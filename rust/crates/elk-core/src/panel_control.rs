//! Port of PanelControl.cs: remote-control commands (bypass, arm/disarm, silence).

use crate::elk_session::ElkSession;
use crate::error::{ElkError, Result};
use crate::panel_live::PanelLive;

/// Remote-control commands from ElkRP's "Current Status and Remote Control" window. Each is one request; the reply is returned as
/// received (its opcode is not fixed) or `None` when the panel stays silent. The caller should re-read the status to see the result.
pub struct PanelControl;

/// Arm level names, index = level.
pub const ARM_LEVELS: [&str; 7] = ["Disarm", "Arm Away", "Arm Stay", "Arm Stay Instant", "Arm Night", "Arm Night Instant", "Arm Vacation"];

impl PanelControl {
    fn send(s: &mut ElkSession, payload: &[u8]) -> Result<Option<Vec<u8>>> {
        s.send(payload)?;
        match s.receive_any() {
            Ok(r) => Ok(Some(r)),
            Err(e) if e.is_timeout() => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Toggles the bypass of one zone (`FF 00 00 09 00 zone area 00`).
    pub fn toggle_bypass(s: &mut ElkSession, zone: i32, area: i32) -> Result<Option<Vec<u8>>> {
        if !(1..=208).contains(&zone) || !(1..=8).contains(&area) {
            return Err(ElkError::OutOfRange("zone or area out of range".into()));
        }
        Self::send(s, &[0xFF, 0, 0, 9, 0, zone as u8, area as u8, 0])
    }

    /// Arms the area at a level 1-6 (away, stay, stay instant, night, night instant, vacation) or disarms it (level 0): `7F 00 40 00 area level`.
    pub fn arm(s: &mut ElkSession, area: i32, level: i32) -> Result<Option<Vec<u8>>> {
        if !(1..=8).contains(&area) || !(0..=6).contains(&level) {
            return Err(ElkError::OutOfRange("area or level out of range".into()));
        }
        Self::send(s, &[0x7F, 0, 0x40, 0, area as u8, level as u8])
    }

    /// Arms only when the area is fully ready, like a keypad that warns about open zones before it will arm. The panel itself will
    /// "force arm" an area whose open zones are force-armable and quietly bypasses them; this refuses that case, and the not-ready case,
    /// and says which zones are open. Disarming (level 0) and an area that is already armed are passed straight through.
    /// `force` skips the check.
    pub fn arm_checked(s: &mut ElkSession, area: i32, level: i32, force: bool) -> Result<Option<Vec<u8>>> {
        if !(1..=8).contains(&area) || !(0..=6).contains(&level) {
            return Err(ElkError::OutOfRange("area or level out of range".into()));
        }
        if level > 0 && !force {
            let snap = PanelLive::read(s)?;
            if let Some(a) = snap.areas.iter().find(|x| x.area == area) {
                if a.armed == 0 && !(a.ready && !a.ready_to_force_arm) {
                    let open: Vec<String> = snap.zones.iter().filter(|z| z.state.starts_with("Violated") && !z.bypassed).map(|z| z.zone.to_string()).take(12).collect();
                    let why = if a.ready_to_force_arm { "it could only be force armed, which bypasses the open zones" } else { "a zone is open or in trouble" };
                    let mut msg = format!("Area {area} is not ready to arm ({why}). ");
                    if !open.is_empty() {
                        msg.push_str(&format!("Open or violated zones: {}. ", open.join(", ")));
                    }
                    msg.push_str("Close them (or bypass them yourself) and arm again, or use force arm to arm anyway (the panel then bypasses them). Nothing was sent.");
                    return Err(ElkError::InvalidOperation(msg));
                }
            }
        }
        Self::arm(s, area, level)
    }

    /// Silences and resets the alarm in an area: `FF 00 00 0A area`.
    pub fn silence(s: &mut ElkSession, area: i32) -> Result<Option<Vec<u8>>> {
        if !(1..=8).contains(&area) {
            return Err(ElkError::OutOfRange("area out of range".into()));
        }
        Self::send(s, &[0xFF, 0, 0, 10, area as u8])
    }
}
