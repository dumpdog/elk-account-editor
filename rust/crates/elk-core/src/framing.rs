//! Port of Framing.cs: CRC-16, wire framing, the streaming frame parser and the AES layer.

use crate::error::{ElkError, Result};
use aes::cipher::generic_array::GenericArray;
use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit};
use aes::{Aes128, Aes256};

/// CRC-16, polynomial 0x1021, init 0, MSB first. Used for wire frames and stored record CRCs.
pub struct Crc16;

impl Crc16 {
    /// CRC of `data`.
    pub fn compute(data: &[u8]) -> u16 {
        let mut crc: u32 = 0;
        for &b in data {
            crc = Self::add(crc, b);
        }
        crc as u16
    }

    fn add(mut crc: u32, b: u8) -> u32 {
        crc ^= (b as u32) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { ((crc << 1) ^ 0x1021) & 0xFFFF } else { (crc << 1) & 0xFFFF };
        }
        crc
    }
}

/// Frame: `10 02 | payload (0x10 doubled) | 10 03 | crc_hi crc_lo`.
pub struct Framing;

const DLE: u8 = 0x10;

impl Framing {
    /// Frames `payload` for the wire.
    pub fn encode(payload: &[u8]) -> Vec<u8> {
        let mut o = Vec::with_capacity(payload.len() + 8);
        o.push(DLE);
        o.push(0x02);
        for &b in payload {
            o.push(b);
            if b == DLE {
                o.push(DLE);
            }
        }
        let crc = Crc16::compute(payload);
        o.push(DLE);
        o.push(0x03);
        o.push((crc >> 8) as u8);
        o.push((crc & 0xFF) as u8);
        o
    }
}

/// Kind of a parsed frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    /// A framed message.
    Message,
    /// `10 06`.
    Ack,
    /// `10 15`.
    Nak,
}

/// One parsed frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Message, ACK or NAK.
    pub kind: FrameKind,
    /// Unescaped payload (`10 06` / `10 15` for ACK / NAK).
    pub payload: Vec<u8>,
    /// Whether the frame CRC matched.
    pub crc_ok: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum S {
    Idle,
    Dle1,
    Body,
    Dle2,
    Crc1,
    Crc2,
}

/// Streaming parser that follows NetAES.ParseM1Msg: `10 06` = ACK, `10 15` = NAK,
/// a body starting with 0xFF is debug text.
pub struct FrameParser {
    s: S,
    buf: Vec<u8>,
    debug_text: bool,
    crc_hi: u8,
    ff_is_debug_text: bool,
}

impl Default for FrameParser {
    fn default() -> Self {
        Self::new(true)
    }
}

impl FrameParser {
    /// `ff_is_debug_text` true (the C# default) for the PC side: a panel frame whose body starts with 0xFF is debug text.
    /// Pass false to parse what a PC sends to the panel, where 0xFF starts real write requests.
    pub fn new(ff_is_debug_text: bool) -> Self {
        FrameParser { s: S::Idle, buf: Vec::new(), debug_text: false, crc_hi: 0, ff_is_debug_text }
    }

    fn reset(&mut self) {
        self.buf.clear();
        self.debug_text = false;
    }

    /// Feeds received bytes, returns the frames completed by them.
    pub fn feed(&mut self, data: &[u8]) -> Vec<Frame> {
        let mut frames = Vec::new();
        let ack = || Frame { kind: FrameKind::Ack, payload: vec![0x10, 0x06], crc_ok: true };
        let nak = || Frame { kind: FrameKind::Nak, payload: vec![0x10, 0x15], crc_ok: true };
        for &b in data {
            match self.s {
                S::Idle => {
                    if b == 0x10 {
                        self.s = S::Dle1;
                    }
                }
                S::Dle1 => {
                    if b == 0x02 {
                        self.reset();
                        self.s = S::Body;
                    } else if b == 0x06 {
                        frames.push(ack());
                        self.s = S::Idle;
                    } else if b == 0x15 {
                        frames.push(nak());
                        self.s = S::Idle;
                    } else {
                        self.s = S::Idle;
                    }
                }
                S::Body => {
                    if b == 0xFF && self.buf.is_empty() && self.ff_is_debug_text {
                        self.debug_text = true;
                    } else if b == 0x10 {
                        self.s = S::Dle2;
                    } else if !self.debug_text {
                        self.buf.push(b);
                    }
                }
                S::Dle2 => {
                    if b == 0x10 {
                        if !self.debug_text {
                            self.buf.push(0x10);
                        }
                        self.s = S::Body;
                    } else if b == 0x02 {
                        self.reset();
                        self.s = S::Body;
                    } else if b == 0x03 {
                        self.s = S::Crc1;
                    } else if b == 0x06 {
                        frames.push(ack());
                        self.s = S::Idle;
                    } else if b == 0x15 {
                        frames.push(nak());
                        self.s = S::Idle;
                    } else {
                        self.s = S::Idle;
                    }
                }
                S::Crc1 => {
                    self.crc_hi = b;
                    self.s = S::Crc2;
                }
                S::Crc2 => {
                    if !self.debug_text {
                        let payload = self.buf.clone();
                        let ok = Crc16::compute(&payload) == (((self.crc_hi as u16) << 8) | b as u16);
                        frames.push(Frame { kind: FrameKind::Message, payload, crc_ok: ok });
                    }
                    self.s = S::Idle;
                }
            }
        }
        frames
    }
}

/// AES layer used when ConnectMethod = 51 (NetAES). Zero padding, ECB, no IV.
pub struct ElkCrypto;

#[allow(clippy::large_enum_variant)]
enum Cipher {
    A128(Aes128),
    A256(Aes256),
}

impl Cipher {
    fn new(key: &[u8]) -> Result<Cipher> {
        match key.len() {
            16 => Ok(Cipher::A128(Aes128::new(GenericArray::from_slice(key)))),
            32 => Ok(Cipher::A256(Aes256::new(GenericArray::from_slice(key)))),
            n => Err(ElkError::Argument(format!("Specified key is not a valid size for this algorithm ({n} bytes)."))),
        }
    }
    #[allow(clippy::chunks_exact_to_as_chunks)]
    fn enc(&self, data: &mut [u8]) {
        for c in data.chunks_exact_mut(16) {
            let b = GenericArray::from_mut_slice(c);
            match self {
                Cipher::A128(a) => a.encrypt_block(b),
                Cipher::A256(a) => a.encrypt_block(b),
            }
        }
    }
    #[allow(clippy::chunks_exact_to_as_chunks)]
    fn dec(&self, data: &mut [u8]) {
        for c in data.chunks_exact_mut(16) {
            let b = GenericArray::from_mut_slice(c);
            match self {
                Cipher::A128(a) => a.decrypt_block(b),
                Cipher::A256(a) => a.decrypt_block(b),
            }
        }
    }
}

impl ElkCrypto {
    /// Session key: AES-ECB encrypt the ASCII passphrase (zero padded to 16-byte blocks) using the ASCII MAC
    /// address (zero padded or cut to 16 bytes) as key. 1..16 chars gives a 16-byte (AES-128) key, 17..32 gives 32 bytes (AES-256).
    /// Errors for an empty passphrase (.NET: the derived empty array is rejected as a key when first used).
    pub fn derive_key(mac_address: &str, passphrase: &str) -> Result<Vec<u8>> {
        let mut k = [0u8; 16];
        let mac = ascii_bytes(mac_address);
        let n = mac.len().min(16);
        k[..n].copy_from_slice(&mac[..n]);
        let pass = ascii_bytes(passphrase);
        let mut padded = vec![0u8; pass.len().div_ceil(16) * 16];
        padded[..pass.len()].copy_from_slice(&pass);
        Cipher::new(&k)?.enc(&mut padded);
        Ok(padded)
    }

    /// Zero-pads `framed` to whole blocks and encrypts it.
    pub fn encrypt(key: &[u8], framed: &[u8]) -> Result<Vec<u8>> {
        let mut padded = vec![0u8; framed.len().div_ceil(16) * 16];
        padded[..framed.len()].copy_from_slice(framed);
        Cipher::new(key)?.enc(&mut padded);
        Ok(padded)
    }

    /// Decrypts the whole blocks of `cipher` (a trailing partial block is dropped).
    pub fn decrypt(key: &[u8], cipher: &[u8]) -> Result<Vec<u8>> {
        let mut d = cipher[..cipher.len() / 16 * 16].to_vec();
        Cipher::new(key)?.dec(&mut d);
        Ok(d)
    }
}

/// `Encoding.ASCII.GetBytes`: every UTF-16 unit above 0x7F becomes `?`.
pub(crate) fn ascii_bytes(s: &str) -> Vec<u8> {
    s.encode_utf16().map(|u| if u < 0x80 { u as u8 } else { b'?' }).collect()
}
