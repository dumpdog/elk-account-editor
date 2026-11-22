//! Port of Handshake.cs: section ids, the connect request, the identity block and the panel's connect reply.

use crate::error::{ElkError, Result};
use aes::cipher::generic_array::GenericArray;
use aes::cipher::{BlockEncrypt, KeyInit};
use aes::Aes128;

/// Section ids. A section is read with the id, written with id + 128, and its per-item CRC is queried with 0x7E.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Section {
    /// 1
    UserCodes = 1,
    /// 3
    Area = 3,
    /// 4
    Keypad = 4,
    /// 5
    Zone = 5,
    /// 7
    Globals = 7,
    /// 8
    Telephone = 8,
    /// 13
    VoiceMessage = 13,
    /// 14
    X10 = 14,
    /// 15
    WirelessZone = 15,
}

impl From<Section> for u8 {
    fn from(s: Section) -> u8 {
        s as u8
    }
}

/// Connect modes. 49 and 50 append the encrypted identity block; 50 is the M1XEP secure port (TLS).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ConnectMode {
    /// 51
    Aes = 51,
    /// 49
    Secure = 49,
    /// 50
    Tls = 50,
}

/// Session opcodes and request builders.
pub struct Handshake;

impl Handshake {
    /// First byte of session requests and of the connect reply.
    pub const SESSION_OPCODE: u8 = 0x7F;
    /// CRC query opcode.
    pub const CRC_QUERY_OPCODE: u8 = 0x7E;
    /// The fixed AES key embedded in ElkRP (M1Fns.AES128_Encrypt_B).
    pub const FIXED_KEY: [u8; 16] = [5, 24, 65, 5, 25, 59, 9, 25, 84, 3, 10, 94, 6, 7, 97, 100];

    /// Connect request: `7F 00 00 01` then the six RP access-code digits, LAST DIGIT FIRST, each as a value 0-9.
    /// For modes 49/50 a 16-byte identity block is appended (`serial_hex` required). `rng` yields values 1..=255; `None` uses the thread RNG.
    pub fn build_connect(access_code: &str, mode: ConnectMode, serial_hex: Option<&str>, rng: Option<&mut dyn FnMut() -> u8>) -> Result<Vec<u8>> {
        let digits: Vec<char> = access_code.chars().collect();
        if digits.len() != 6 || !digits.iter().all(|c| c.is_ascii_digit()) {
            return Err(ElkError::Argument("RP access code must be 6 digits.".into()));
        }
        let mut p = vec![Self::SESSION_OPCODE, 0, 0, 1];
        for i in (0..6).rev() {
            p.push(digits[i] as u8 - b'0');
        }
        if mode != ConnectMode::Aes {
            let serial = serial_hex.ok_or_else(|| ElkError::Argument("serial required".into()))?;
            let mut default_rng = default_rng();
            let r: &mut dyn FnMut() -> u8 = match rng {
                Some(r) => r,
                None => &mut default_rng,
            };
            p.extend_from_slice(&Self::identity_block(access_code, serial, r)?);
        }
        Ok(p)
    }

    /// 16 bytes: [0] random 1..255 | [1..8] serial number as 8 hex nibbles | [9] random 1..255 | [10..15] access code digits (forward order),
    /// AES-128-CBC with zero IV (one block, so identical to ECB) and the fixed key.
    pub fn identity_block(access_code: &str, serial_hex: &str, rng: &mut dyn FnMut() -> u8) -> Result<[u8; 16]> {
        let serial: Vec<char> = serial_hex.chars().collect();
        if serial.len() != 8 {
            return Err(ElkError::Argument("serial must be 8 hex digits".into()));
        }
        let code: Vec<char> = access_code.chars().collect();
        let mut a = [0u8; 16];
        a[0] = rng();
        for i in 0..8 {
            a[1 + i] = serial[i].to_digit(16).ok_or_else(|| ElkError::Argument("serial must be 8 hex digits".into()))? as u8;
        }
        a[9] = rng();
        for i in 0..6 {
            a[10 + i] = code.get(i).map(|c| (*c as u32).wrapping_sub('0' as u32) as u8).ok_or_else(|| ElkError::Argument("access code".into()))?;
        }
        let aes = Aes128::new(GenericArray::from_slice(&Self::FIXED_KEY));
        aes.encrypt_block(GenericArray::from_mut_slice(&mut a));
        Ok(a)
    }

    /// `7F 00 00 A5`
    pub fn keep_alive() -> Vec<u8> {
        vec![Self::SESSION_OPCODE, 0, 0, 0xA5]
    }
    /// `7F 01 00 A5`
    pub fn disconnect() -> Vec<u8> {
        vec![Self::SESSION_OPCODE, 1, 0, 0xA5]
    }
    /// `[section, part, 0, item]`. `part` is request byte 1; only keypads use it (0 = bytes 0..27, 1 = bytes 28..147).
    pub fn read_request(s: impl Into<u8>, item: u8, part: u8) -> Vec<u8> {
        vec![s.into(), part, 0, item]
    }
    /// `[7E, section, 0, item]`
    pub fn crc_query(s: impl Into<u8>, item: u8) -> Vec<u8> {
        vec![Self::CRC_QUERY_OPCODE, s.into(), 0, item]
    }
    /// `[section + 128, part, 0, item, data...]`
    pub fn write_request(s: impl Into<u8>, item: u8, data: &[u8], part: u8) -> Vec<u8> {
        let mut p = vec![0u8; 4 + data.len()];
        p[0] = s.into().wrapping_add(128);
        p[1] = part;
        p[3] = item;
        p[4..].copy_from_slice(data);
        p
    }
}

fn default_rng() -> impl FnMut() -> u8 {
    use rand::Rng;
    let mut r = rand::thread_rng();
    move || r.gen_range(1..=255u8)
}

/// Panel reply to the connect request (opcode 0x7F). Offsets from DoConnectProtocol:
/// [1] status (0 = accepted) | [4..6] firmware major.minor.patch (when [4] >= 4; below that the panel is in its boot loader)
/// | [8..11] serial number as 8 hex digits | [12..13] hardware version | [14..16] boot version | [22..24] minimum RP version required.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectReply {
    /// Status byte was 0.
    pub accepted: bool,
    /// Firmware major below 4.
    pub in_boot_loader: bool,
    /// `major.minor.patch`, empty in the boot loader.
    pub firmware: String,
    /// 8 upper-case hex digits.
    pub serial_hex: String,
    /// `major.minor`.
    pub hardware: String,
    /// `a.b.c`.
    pub boot: String,
    /// Minimum RP version, when the reply carries one.
    pub min_rp_version: Option<String>,
}

impl ConnectReply {
    /// Parses a connect reply. (C# throws `IndexOutOfRangeException` for exactly 16 bytes; this returns `InvalidData` for it.)
    pub fn parse(p: &[u8]) -> Result<ConnectReply> {
        if p.len() < 16 || p[0] != Handshake::SESSION_OPCODE {
            return Err(ElkError::InvalidData("Not a connect reply.".into()));
        }
        if p.len() < 17 {
            return Err(ElkError::InvalidData("Connect reply too short.".into()));
        }
        let boot = p[4] < 4;
        let serial: String = p[8..12].iter().map(|b| format!("{b:02X}")).collect();
        Ok(ConnectReply {
            accepted: p[1] == 0,
            in_boot_loader: boot,
            firmware: if boot { String::new() } else { format!("{}.{}.{}", p[4], p[5], p[6]) },
            serial_hex: serial,
            hardware: format!("{}.{}", p[12], p[13]),
            boot: format!("{}.{}.{}", p[14], p[15], p[16]),
            min_rp_version: if p.len() >= 25 && p[22] > 0 && p[23] > 0 && p[24] > 1 { Some(format!("{}.{}.{}", p[22], p[23], p[24])) } else { None },
        })
    }
}
