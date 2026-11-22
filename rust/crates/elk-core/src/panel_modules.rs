//! Port of PanelModules.cs: bus module enrollment and the installed-device list.

use crate::elk_session::ElkSession;
use crate::error::hex_upper;
use crate::error::Result;
use crate::framing::FrameKind;

/// One device on the M1 data bus (or the control itself) with the versions it reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusModule {
    /// Device kind.
    pub kind: String,
    /// Bus address (1-based, 0 for the control and the M1XEP).
    pub address: i32,
    /// Model name.
    pub model: String,
    /// Hardware version.
    pub hardware: String,
    /// Boot version.
    pub boot: String,
    /// Firmware version or a message.
    pub firmware: String,
    /// Whether the device answered.
    pub responding: bool,
}

/// Bus module enrollment and the installed-device list, from ElkRP's "Enroll/Update Control and Devices" window.
pub struct PanelModules;

/// Kind, device type used in the version query, first item in ElkRP's ValidItems table, number of devices.
const KINDS: [(&str, u8, i32, i32); 4] = [("Keypad", 1, 0, 16), ("Input expander", 2, 1, 13), ("Output expander", 3, 2, 13), ("Serial port expander", 5, 4, 7)];

fn keypad_model(hw: i32) -> &'static str {
    match hw {
        i32::MIN..=255 => "M1KP",
        256..=511 => "M1KP2/KP3",
        512..=767 => "M1KPAS",
        768..=1023 => "M1KAM",
        1024..=1279 => "M1KPNAV",
        1280..=1535 => "P212S",
        _ => "Unknown",
    }
}

fn input_model(hw: i32) -> &'static str {
    match hw {
        i32::MIN..=255 => "M1XIN",
        256..=511 => "M1XRF2G",
        512..=767 => "M1XRF1N",
        768..=1023 => "M1XRF2N",
        1024..=1279 => "M1XRF2H",
        1280..=1535 => "M1XRFEG",
        1536..=1791 => "M1XRFTW",
        1792..=2047 => "M1XRFTWM",
        2048..=2303 => "M1XRF319",
        2304..=2559 => "M1XRF2Hv2",
        _ => "Unknown",
    }
}

fn serial_model(hw: i32) -> &'static str {
    if (2560..=2815).contains(&hw) {
        "M1XSLU"
    } else if (2816..=3071).contains(&hw) {
        "M1XSLZW"
    } else {
        "M1XSP"
    }
}

fn ver3(v: i32) -> String {
    format!("{}.{}.{}", v / 65536, (v / 256) & 0xFF, v & 0xFF)
}

impl PanelModules {
    /// Which devices the panel has enrolled, by kind, device type and address (1-based).
    /// Group g of the reply holds 32 devices, highest byte first.
    pub fn decode_enrolled(reply: &[u8]) -> Vec<(&'static str, u8, i32)> {
        // ElkRP picks the first byte of the groups from the reply length (SystemVers.GetActiveExps)
        let len = reply.len();
        let num: isize = if len <= 76 {
            47
        } else if len <= 88 {
            59
        } else if len <= 171 {
            147
        } else {
            151
        };
        let mut list = Vec::new();
        for (kind, type_, group, count) in KINDS {
            for n in 1..=count {
                let b = (n - 1) as isize;
                let idx = num + 4 + 4 * group as isize - b / 8;
                if idx >= 0 && (idx as usize) < len && (reply[idx as usize] & (1 << (b % 8))) != 0 {
                    list.push((kind, type_, n));
                }
            }
        }
        list
    }

    /// Model name for a device kind and its hardware number.
    pub fn model_for(kind: &str, hardware: i32) -> &'static str {
        match kind {
            "Keypad" => keypad_model(hardware),
            "Input expander" => input_model(hardware),
            "Output expander" => {
                if hardware <= 255 {
                    "M1XOV(R)"
                } else {
                    "Unknown"
                }
            }
            "Serial port expander" => serial_model(hardware),
            "Ethernet expander" => "M1XEP",
            _ => "Unknown",
        }
    }

    /// Decodes a device version reply (7C or 75): hardware r[8..9], boot r[10..12], firmware r[13..15].
    pub fn decode_versions(kind: &str, address: i32, r: Option<&[u8]>) -> BusModule {
        let r = match r {
            Some(r) if r.len() >= 16 => r,
            _ => {
                return BusModule {
                    kind: kind.into(),
                    address,
                    model: "Unknown".into(),
                    hardware: String::new(),
                    boot: String::new(),
                    firmware: "Missing or not working".into(),
                    responding: false,
                }
            }
        };
        let hw = r[8] as i32 * 256 + r[9] as i32;
        let boot = r[10] as i32 * 65536 + r[11] as i32 * 256 + r[12] as i32;
        let fw = r[13] as i32 * 65536 + r[14] as i32 * 256 + r[15] as i32;
        let firm = if fw == 16777215 {
            "Missing or not working".to_string()
        } else if fw == 0 {
            "Erased".to_string()
        } else {
            ver3(fw)
        };
        BusModule {
            kind: kind.into(),
            address,
            model: Self::model_for(kind, hw).into(),
            hardware: format!("{}.{}", (hw / 256) & 0xFF, hw & 0xFF),
            boot: ver3(boot),
            firmware: firm,
            responding: fw != 16777215,
        }
    }

    fn ask(s: &mut ElkSession, request: &[u8], expect: u8) -> Result<Option<Vec<u8>>> {
        s.send(request)?;
        match s.receive(expect) {
            Ok(r) => Ok(Some(r)),
            Err(e) if e.is_timeout() => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Reads the control, the M1XEP and every enrolled device with its versions. Read only.
    pub fn scan(s: &mut ElkSession) -> Result<Vec<BusModule>> {
        let mut list = Vec::new();
        if let Some(p) = &s.panel {
            list.push(BusModule {
                kind: "Control".into(),
                address: 0,
                model: "M1G".into(),
                hardware: p.hardware.clone(),
                boot: p.boot.clone(),
                firmware: p.firmware.clone(),
                responding: true,
            });
        }
        s.send(&[0x7F, 0, 0, 4])?;
        let enrolled = s.receive(0x7F)?;
        for (kind, type_, addr) in Self::decode_enrolled(&enrolled) {
            let mut r = Self::ask(s, &[0x7C, 0, type_, addr as u8], 0x7C)?;
            if let Some(x) = &r {
                if x.get(2) != Some(&type_) || x.get(3) != Some(&(addr as u8)) {
                    r = None;
                }
            }
            list.push(Self::decode_versions(kind, addr, r.as_deref()));
        }
        let xep = Self::ask(s, &[0x75, 6], 0x75)?;
        if let Some(x) = xep {
            if x.len() >= 16 {
                list.push(Self::decode_versions("Ethernet expander", 0, Some(&x)));
            }
        }
        Ok(list)
    }

    /// Starts bus module enrollment. The panel drops the connection and rescans the bus for several seconds; log in again afterwards
    /// (about 10 seconds later). Devices that are enrolled but missing at that moment are not kept.
    pub fn enroll(s: &mut ElkSession) -> Result<String> {
        s.send(&[0x7F, 0, 0, 17])?;
        match s.receive_any_frame() {
            Ok(f) => Ok(match f.kind {
                FrameKind::Ack => "the panel acknowledged (ACK)".to_string(),
                FrameKind::Nak => "the panel refused (NAK)".to_string(),
                FrameKind::Message => format!("the panel answered: {}", hex_upper(&f.payload)),
            }),
            Err(e) if e.is_timeout() => Ok("the panel sent no answer".to_string()),
            Err(e) if e.is_io() => Ok("the panel closed the connection".to_string()),
            Err(e) => Err(e),
        }
    }
}
