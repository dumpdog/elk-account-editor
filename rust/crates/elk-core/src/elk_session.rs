//! Port of ElkSession.cs: a blocking request/response session over plain TCP, AES-over-TCP or TLS (M1XEP secure port).
//!
//! Decision: blocking `std::net` with read timeouts, no async runtime. The C# `CancellationToken` parameters are dropped;
//! a caller that needs to abort sets a shorter [`ElkSession::timeout`] or drops the session from another thread's socket close.

use crate::error::{ElkError, Result};
use crate::framing::{ElkCrypto, Frame, FrameKind, FrameParser, Framing};
use crate::handshake::{ConnectMode, ConnectReply, Handshake, Section};
use crate::records::{AreaRecord, KeypadRecord, ZoneRecord};
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

/// Issuers of the M1XEP's self-signed certificate that ElkRP itself accepts.
pub const TRUSTED_ISSUERS: [&str; 2] = ["CN=M1XEP, OU=Engineering, O=Elk, L=Hildebran, S=NC, C=US", "CN=HTTPS Certificate"];

enum Net {
    Plain(TcpStream),
    Tls(Box<native_tls::TlsStream<TcpStream>>),
}

impl Net {
    fn tcp(&self) -> &TcpStream {
        match self {
            Net::Plain(s) => s,
            Net::Tls(s) => s.get_ref(),
        }
    }
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Net::Plain(s) => s.read(buf),
            Net::Tls(s) => s.read(buf),
        }
    }
    fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        match self {
            Net::Plain(s) => {
                s.write_all(data)?;
                s.flush()
            }
            Net::Tls(s) => {
                s.write_all(data)?;
                s.flush()
            }
        }
    }
}

/// Request/response session with the panel (ElkRP ConnectMethod 48 plain, 51 AES, 50 TLS).
/// Tested only against the in-process simulated panel, not against real hardware (as in C#), except that the C# TLS path was used on a real panel.
pub struct ElkSession {
    net: Option<Net>,
    tls: bool,
    serial_hex: Option<String>,
    key: Option<Vec<u8>>, // None = plain network or TLS
    parser: FrameParser,
    frames: VecDeque<Frame>,
    cipher_buf: Vec<u8>,
    /// The panel's connect reply, set by [`ElkSession::login`].
    pub panel: Option<ConnectReply>,
    /// How long a receive waits for the reply (default 3000 ms).
    pub timeout: Duration,
    /// Called after every record or block read or written (the progress counter hangs on this).
    pub on_transfer: Option<Box<dyn FnMut() + Send>>,
}

impl ElkSession {
    fn blank(key: Option<Vec<u8>>, tls: bool, serial_hex: Option<String>) -> Self {
        ElkSession {
            net: None,
            tls,
            serial_hex,
            key,
            parser: FrameParser::default(),
            frames: VecDeque::new(),
            cipher_buf: Vec::new(),
            panel: None,
            timeout: Duration::from_millis(3000),
            on_transfer: None,
        }
    }

    /// AES session (ConnectMethod 51), key derived from the MAC address and passphrase. Errors for an empty passphrase.
    pub fn new(mac_address: &str, passphrase: &str) -> Result<Self> {
        let key = ElkCrypto::derive_key(mac_address, passphrase)?;
        // validate the key size now (C# fails on first use)
        ElkCrypto::encrypt(&key, &[])?;
        Ok(Self::blank(Some(key), false, None))
    }

    /// Plain (non-secure) network session: the same framing with no AES layer.
    pub fn plain() -> Self {
        Self::blank(None, false, None)
    }

    /// M1XEP secure port (usually 2601): the same framing inside a TLS stream. The login carries a block built from the control's
    /// serial number (8 hex digits). Only the two issuers ElkRP itself accepts are trusted.
    pub fn tls(serial_hex: &str) -> Result<Self> {
        if serial_hex.chars().count() == 8 && serial_hex.chars().all(|c| c.is_ascii_hexdigit()) {
            Ok(Self::blank(None, true, Some(serial_hex.to_string())))
        } else {
            Err(ElkError::Argument("The panel serial number must be 8 hex digits.".into()))
        }
    }

    /// Opens the TCP connection (and the TLS handshake for a TLS session).
    pub fn connect(&mut self, host: &str, port: u16) -> Result<()> {
        let tcp = TcpStream::connect((host, port))?;
        let _ = tcp.set_nodelay(true);
        if !self.tls {
            self.net = Some(Net::Plain(tcp));
            return Ok(());
        }
        let connector = native_tls::TlsConnector::builder()
            .min_protocol_version(Some(native_tls::Protocol::Tlsv12))
            .max_protocol_version(Some(native_tls::Protocol::Tlsv12))
            // The module's certificate is self-signed: chain and host name checks are replaced by the issuer check below.
            .danger_accept_invalid_certs(true)
            .danger_accept_invalid_hostnames(true)
            .use_sni(true)
            .build()
            .map_err(|e| tls_error(&e.to_string()))?;
        let stream = connector.connect("M1XEP", tcp).map_err(|e| tls_error(&e.to_string()))?;
        let cert = stream.peer_certificate().map_err(|e| tls_error(&e.to_string()))?.ok_or_else(|| tls_error("the panel sent no certificate"))?;
        let der = cert.to_der().map_err(|e| tls_error(&e.to_string()))?;
        let issuer = issuer_string(&der).ok_or_else(|| tls_error("the panel certificate could not be read"))?;
        if !TRUSTED_ISSUERS.contains(&issuer.as_str()) {
            return Err(tls_error(&format!("untrusted certificate issuer {issuer}")));
        }
        self.net = Some(Net::Tls(Box::new(stream)));
        Ok(())
    }

    /// Connect then log in with the six-digit RP access code. Errors if the panel does not accept it.
    pub fn login(&mut self, access_code: &str) -> Result<ConnectReply> {
        let req = if self.tls {
            Handshake::build_connect(access_code, ConnectMode::Tls, self.serial_hex.as_deref(), None)?
        } else {
            Handshake::build_connect(access_code, ConnectMode::Aes, None, None)?
        };
        self.send(&req)?;
        let reply = ConnectReply::parse(&self.receive(Handshake::SESSION_OPCODE)?)?;
        if !reply.accepted {
            return Err(ElkError::Unauthorized("Panel rejected the RP access code.".into()));
        }
        self.panel = Some(reply.clone());
        Ok(reply)
    }

    /// Frames (and encrypts) and sends one payload.
    pub fn send(&mut self, payload: &[u8]) -> Result<()> {
        let framed = Framing::encode(payload);
        let data = match &self.key {
            None => framed,
            Some(k) => ElkCrypto::encrypt(k, &framed)?,
        };
        let net = self.net.as_mut().ok_or_else(not_connected)?;
        net.write_all(&data)?;
        Ok(())
    }

    /// Wait for a good message whose first byte equals `expect`. Partial AES blocks are buffered across reads.
    pub fn receive(&mut self, expect: u8) -> Result<Vec<u8>> {
        let what = format!("opcode 0x{expect:02X}");
        Ok(self.receive_frame_where(|f| f.kind == FrameKind::Message && f.crc_ok && !f.payload.is_empty() && f.payload[0] == expect, &what)?.payload)
    }

    /// Wait for the next good message whatever its first byte is (used for the remote-control commands, whose reply opcode is not fixed).
    pub fn receive_any(&mut self) -> Result<Vec<u8>> {
        Ok(self.receive_frame_where(|f| f.kind == FrameKind::Message && f.crc_ok && !f.payload.is_empty(), "any message")?.payload)
    }

    /// Wait for the next frame of any kind: a message, or the bare acknowledgement (10 06) / refusal (10 15) the panel sends for some commands.
    pub fn receive_any_frame(&mut self) -> Result<Frame> {
        self.receive_frame_where(|_| true, "any frame")
    }

    fn receive_frame_where(&mut self, accept: impl Fn(&Frame) -> bool, what: &str) -> Result<Frame> {
        let deadline = Instant::now() + self.timeout;
        let mut buf = [0u8; 2048];
        loop {
            while let Some(f) = self.frames.pop_front() {
                if accept(&f) {
                    return Ok(f);
                }
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(ElkError::Timeout(format!("No reply for {what}.")));
            }
            let net = self.net.as_mut().ok_or_else(not_connected)?;
            net.tcp().set_read_timeout(Some((deadline - now).max(Duration::from_millis(1))))?;
            let n = match net.read(&mut buf) {
                Ok(n) => n,
                Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
                    return Err(ElkError::Timeout(format!("No reply for {what}.")));
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.into()),
            };
            if n == 0 {
                return Err(ElkError::Io(io::Error::new(io::ErrorKind::UnexpectedEof, "Connection closed by panel.")));
            }
            match &self.key {
                None => self.frames.extend(self.parser.feed(&buf[..n])),
                Some(key) => {
                    self.cipher_buf.extend_from_slice(&buf[..n]);
                    let whole = self.cipher_buf.len() / 16 * 16;
                    if whole == 0 {
                        continue;
                    }
                    let chunk: Vec<u8> = self.cipher_buf.drain(..whole).collect();
                    let plain = ElkCrypto::decrypt(key, &chunk)?;
                    self.frames.extend(self.parser.feed(&plain));
                }
            }
        }
    }

    fn transferred(&mut self) {
        if let Some(f) = self.on_transfer.as_mut() {
            f();
        }
    }

    /// Read one record. Request `[section,part,0,item]`; the reply echoes the section id, record bytes start at offset 4.
    pub fn read_record(&mut self, s: impl Into<u8>, item: u8, size: usize, part: u8) -> Result<Vec<u8>> {
        let s = s.into();
        self.send(&Handshake::read_request(s, item, part))?;
        let r = self.receive(s)?;
        if r.len() < 4 + size {
            return Err(ElkError::InvalidData(format!("Short reply ({} bytes, wanted {}).", r.len(), 4 + size)));
        }
        self.transferred();
        Ok(r[4..4 + size].to_vec())
    }

    /// Write one record. Request `[section+128,part,0,item,data...]`; the panel echoes section+128.
    pub fn write_record(&mut self, s: impl Into<u8>, item: u8, data: &[u8], part: u8) -> Result<()> {
        let s = s.into();
        self.send(&Handshake::write_request(s, item, data, part))?;
        self.receive(s.wrapping_add(128))?;
        self.transferred();
        Ok(())
    }

    /// Read any section item. Returns everything after the 4-byte reply header `[section][part][0][item]`.
    pub fn read_block(&mut self, section: u8, item: u8, part: u8) -> Result<Vec<u8>> {
        self.send(&[section, part, 0, item])?;
        let r = self.receive(section)?;
        self.transferred();
        if r.len() < 4 {
            return Err(ElkError::InvalidData(format!("Short reply ({} bytes).", r.len())));
        }
        Ok(r[4..].to_vec())
    }

    /// Write any section item (opcode = section + 128). The panel echoes the opcode.
    pub fn write_block(&mut self, section: u8, item: u8, data: &[u8], part: u8) -> Result<()> {
        self.send(&Handshake::write_request(section, item, data, part))?;
        self.receive(section.wrapping_add(128))?;
        self.transferred();
        Ok(())
    }

    /// Read with a 16-bit item number: `[section, part, item_hi, item_lo]` (automation items: `part` is a count for lights, voice messages and rule
    /// packets, a sub-section for names, tasks and counters). The reply must echo the three bytes after the section, like ElkRP's `MoreExpected`
    /// check, and carry at least `min_len` data bytes. Returns everything after the 4-byte header.
    pub fn read_block16(&mut self, section: u8, part: u8, item: u16, min_len: usize) -> Result<Vec<u8>> {
        self.send(&[section, part, (item >> 8) as u8, (item & 0xFF) as u8])?;
        let r = self.receive(section)?;
        self.transferred();
        if r.len() < 4 + min_len {
            return Err(ElkError::InvalidData(format!("Short reply ({} bytes, wanted {}).", r.len(), 4 + min_len)));
        }
        if r[1] != part || r[2] != (item >> 8) as u8 || r[3] != (item & 0xFF) as u8 {
            return Err(ElkError::InvalidData(format!("The panel answered a different item (section {section}, item {item}).")));
        }
        Ok(r[4..].to_vec())
    }

    /// Write with a 16-bit item number: `[section+128, part, item_hi, item_lo, data...]`; the reply must echo the header.
    pub fn write_block16(&mut self, section: u8, part: u8, item: u16, data: &[u8]) -> Result<()> {
        let op = section.wrapping_add(128);
        let mut p = vec![op, part, (item >> 8) as u8, (item & 0xFF) as u8];
        p.extend_from_slice(data);
        self.send(&p)?;
        let r = self.receive(op)?;
        self.transferred();
        if r.len() < 4 || r[1] != part || r[2] != (item >> 8) as u8 || r[3] != (item & 0xFF) as u8 {
            return Err(ElkError::InvalidData(format!("The panel did not confirm the write (section {section}, item {item}).")));
        }
        Ok(())
    }

    /// The panel's stored CRC for an item (ElkRP compares it with its ModCRC/UDCRC to skip unchanged records).
    pub fn query_crc(&mut self, s: impl Into<u8>, item: u8) -> Result<u16> {
        self.send(&Handshake::crc_query(s, item))?;
        let r = self.receive(Handshake::CRC_QUERY_OPCODE)?;
        if r.len() < 6 {
            return Err(ElkError::InvalidData(format!("Short reply ({} bytes).", r.len())));
        }
        Ok(((r[4] as u16) << 8) | r[5] as u16)
    }

    /// Reads one zone record.
    pub fn read_zone(&mut self, zone: u8) -> Result<ZoneRecord> {
        Ok(ZoneRecord::from_bytes(&self.read_record(Section::Zone, zone, ZoneRecord::SIZE, 0)?))
    }

    /// Reads one area record.
    pub fn read_area(&mut self, area: u8) -> Result<AreaRecord> {
        let r = self.read_record(Section::Area, area, AreaRecord::SIZE, 0)?;
        let mut a = AreaRecord::new();
        a.bytes.copy_from_slice(&r);
        Ok(a)
    }

    /// Keypads move in two parts: part 0 = 28 bytes, part 1 = 120 bytes.
    pub fn read_keypad(&mut self, keypad: u8) -> Result<KeypadRecord> {
        let mut k = KeypadRecord::new();
        let a = self.read_record(Section::Keypad, keypad, 28, 0)?;
        k.bytes[..28].copy_from_slice(&a);
        let b = self.read_record(Section::Keypad, keypad, 120, 1)?;
        k.bytes[28..].copy_from_slice(&b);
        Ok(k)
    }

    /// Writes a keypad record in its two parts.
    pub fn write_keypad(&mut self, keypad: u8, k: &KeypadRecord) -> Result<()> {
        self.write_record(Section::Keypad, keypad, &k.bytes[..28], 0)?;
        self.write_record(Section::Keypad, keypad, &k.bytes[28..], 1)
    }

    /// Sends the keep-alive.
    pub fn keep_alive(&mut self) -> Result<()> {
        self.send(&Handshake::keep_alive())
    }

    /// Sends the disconnect request; I/O errors are ignored.
    pub fn disconnect(&mut self) -> Result<()> {
        match self.send(&Handshake::disconnect()) {
            Err(ElkError::Io(_)) => Ok(()),
            other => other,
        }
    }
}

fn not_connected() -> ElkError {
    ElkError::InvalidOperation("The session is not connected.".into())
}

fn tls_error(msg: &str) -> ElkError {
    ElkError::Io(io::Error::other(format!("TLS handshake with the panel failed: {msg}")))
}

/// The issuer of a DER certificate in the .NET `X509Certificate.Issuer` form: RDNs most specific first, joined by `, `,
/// state written as `S`. Returns `None` when the certificate cannot be parsed.
pub fn issuer_string(der: &[u8]) -> Option<String> {
    use x509_parser::prelude::*;
    let (_, cert) = X509Certificate::from_der(der).ok()?;
    let mut parts: Vec<String> = Vec::new();
    for rdn in cert.issuer().iter() {
        let mut attrs: Vec<String> = Vec::new();
        for a in rdn.iter() {
            let oid = a.attr_type().to_id_string();
            let key = match oid.as_str() {
                "2.5.4.3" => "CN".to_string(),
                "2.5.4.11" => "OU".to_string(),
                "2.5.4.10" => "O".to_string(),
                "2.5.4.7" => "L".to_string(),
                "2.5.4.8" => "S".to_string(),
                "2.5.4.6" => "C".to_string(),
                other => format!("OID.{other}"),
            };
            let value = a.as_str().map(|s| s.to_string()).unwrap_or_default();
            attrs.push(format!("{key}={value}"));
        }
        parts.push(attrs.join(" + "));
    }
    parts.reverse();
    Some(parts.join(", "))
}
