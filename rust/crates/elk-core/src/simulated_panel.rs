//! Port of tests\Elk.Tests\SimulatedPanel.cs: a software stand-in for a panel that follows the protocol as traced from ElkRP's code.
//!
//! It exercises framing, AES, partial-block reads and the request/response formats. It cannot prove the real panel agrees, so passing tests
//! mean "consistent with ElkRP's code", not "works on hardware". It is public so that other crates (PanelSync tests) can use it.
//! Differences from the C#: records are stored under `(section id, item)` as bytes, a plain (no AES) mode exists, it accepts further
//! connections after the first one ended, and requests for records that were never stored get no reply instead of killing the server.

use crate::framing::{ElkCrypto, FrameKind, FrameParser, Framing};
use crate::handshake::Section;
use chrono::{Datelike, NaiveDate, NaiveDateTime, Timelike};
use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::Duration;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

struct Shared {
    key: Option<Vec<u8>>,
    access_code: Mutex<String>,
    clock: Mutex<NaiveDateTime>,
    log_records: Mutex<Vec<Vec<u8>>>,
    records: Mutex<HashMap<(u8, u8), Vec<u8>>>,
    auto_items: Mutex<HashMap<(u8, u8, u16), Vec<u8>>>,
    packets: Mutex<Vec<u8>>,
    received: Mutex<Vec<Vec<u8>>>,
    split_writes: AtomicBool,
    arm_requests: Mutex<Vec<Vec<u8>>>,
    area_status: Mutex<HashMap<i32, u8>>,
    xep: Mutex<crate::xep_config::XepSim>,
    stop: AtomicBool,
}

enum Reply {
    None,
    Ack,
    Msg(Vec<u8>),
}

/// The in-process simulated panel (see the module documentation).
pub struct SimulatedPanel {
    shared: Arc<Shared>,
    port: u16,
    handle: Option<JoinHandle<()>>,
}

fn bcd(v: i32) -> u8 {
    (((v / 10) << 4) | (v % 10)) as u8
}
fn unbcd(b: u8) -> i32 {
    ((b >> 4) as i32) * 10 + (b & 15) as i32
}

impl SimulatedPanel {
    /// AES panel (ConnectMethod 51) on a free loopback port.
    pub fn new(mac: &str, passphrase: &str, access_code: &str) -> Self {
        let key = ElkCrypto::derive_key(mac, passphrase).expect("valid passphrase");
        Self::start(Some(key), access_code)
    }

    /// Plain (non-secure) panel: framing only.
    pub fn new_plain(access_code: &str) -> Self {
        Self::start(None, access_code)
    }

    fn start(key: Option<Vec<u8>>, access_code: &str) -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind loopback");
        listener.set_nonblocking(true).expect("nonblocking");
        let port = listener.local_addr().expect("addr").port();
        let shared = Arc::new(Shared {
            key,
            access_code: Mutex::new(access_code.to_string()),
            clock: Mutex::new(NaiveDate::from_ymd_opt(2026, 11, 12).unwrap().and_hms_opt(19, 45, 30).unwrap()),
            log_records: Mutex::new(Vec::new()),
            records: Mutex::new(HashMap::new()),
            auto_items: Mutex::new(HashMap::new()),
            packets: Mutex::new(vec![0u8; 528 * 8]),
            received: Mutex::new(Vec::new()),
            split_writes: AtomicBool::new(false),
            arm_requests: Mutex::new(Vec::new()),
            area_status: Mutex::new(HashMap::new()),
            xep: Mutex::new(crate::xep_config::XepSim::default()),
            stop: AtomicBool::new(false),
        });
        let sh = shared.clone();
        let handle = thread::spawn(move || serve(listener, sh));
        SimulatedPanel { shared, port, handle: Some(handle) }
    }

    /// The loopback port to connect to.
    pub fn port(&self) -> u16 {
        self.port
    }
    /// The current RP access code.
    pub fn access_code(&self) -> String {
        lock(&self.shared.access_code).clone()
    }
    /// The panel clock.
    pub fn clock(&self) -> NaiveDateTime {
        *lock(&self.shared.clock)
    }
    /// Sets the panel clock.
    pub fn set_clock(&self, t: NaiveDateTime) {
        *lock(&self.shared.clock) = t;
    }
    /// Splits every reply into two writes (5 bytes, a pause, the rest) to exercise partial-block reads.
    pub fn set_split_writes(&self, on: bool) {
        self.shared.split_writes.store(on, Ordering::SeqCst);
    }
    /// Adds a packed 8-byte log record (newest first).
    pub fn push_log_record(&self, rec: Vec<u8>) {
        lock(&self.shared.log_records).push(rec);
    }
    /// Stores a record (what `Records[(section, item)] = data` does in C#).
    pub fn set_record(&self, section: impl Into<u8>, item: u8, data: Vec<u8>) {
        lock(&self.shared.records).insert((section.into(), item), data);
    }
    /// A stored record.
    pub fn record(&self, section: impl Into<u8>, item: u8) -> Option<Vec<u8>> {
        lock(&self.shared.records).get(&(section.into(), item)).cloned()
    }
    /// Stores an automation item the way the panel keeps it: section 2 with its part byte (0 custom values, 1 outputs, 2 counters and thermostats,
    /// 3 tasks), section 14 (lighting, item 1-256) or section 13 (voice messages, item 209-323). Unset items read as zeros.
    pub fn set_auto_item(&self, section: u8, part: u8, item: u16, data: Vec<u8>) {
        lock(&self.shared.auto_items).insert((section, auto_part(section, part), item), data);
    }
    /// A stored automation item (None when it was never written or set).
    pub fn auto_item(&self, section: u8, part: u8, item: u16) -> Option<Vec<u8>> {
        lock(&self.shared.auto_items).get(&(section, auto_part(section, part), item)).cloned()
    }
    /// The 528 rule / text packets.
    pub fn packets(&self) -> Vec<[u8; 8]> {
        lock(&self.shared.packets).chunks(8).map(|c| c.try_into().expect("8")).collect()
    }
    /// Stores packets starting at packet `first`.
    pub fn set_packets(&self, first: usize, packets: &[[u8; 8]]) {
        let mut p = lock(&self.shared.packets);
        for (i, pk) in packets.iter().enumerate() {
            p[(first + i) * 8..(first + i + 1) * 8].copy_from_slice(pk);
        }
    }
    /// Every payload received from the client, in order.
    pub fn received(&self) -> Vec<Vec<u8>> {
        lock(&self.shared.received).clone()
    }
    /// The arm / disarm requests received (they get no reply).
    pub fn arm_requests(&self) -> Vec<Vec<u8>> {
        lock(&self.shared.arm_requests).clone()
    }
    /// Area status byte to report instead of the default (bits 2-4: 0 not ready, 4 ready, 8 ready to force arm).
    pub fn set_area_status(&self, area: i32, status: u8) {
        lock(&self.shared.area_status).insert(area, status);
    }
    /// The simulated M1XEP behind opcode 0x75 (setup blocks, firmware, writes received).
    pub fn with_xep<T>(&self, f: impl FnOnce(&mut crate::xep_config::XepSim) -> T) -> T {
        f(&mut lock(&self.shared.xep))
    }
}

impl Drop for SimulatedPanel {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn serve(listener: TcpListener, sh: Arc<Shared>) {
    while !sh.stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_nonblocking(false);
                handle_conn(stream, &sh);
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(5)),
            Err(_) => break,
        }
    }
}

fn handle_conn(mut net: TcpStream, sh: &Shared) {
    let _ = net.set_read_timeout(Some(Duration::from_millis(20)));
    let mut parser = FrameParser::new(false); // the PC sends write requests that start with 0xFF
    let mut buf = [0u8; 2048];
    let mut pending: Vec<u8> = Vec::new();
    while !sh.stop.load(Ordering::SeqCst) {
        let n = match net.read(&mut buf) {
            Ok(0) => return,
            Ok(n) => n,
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => continue,
            Err(_) => return,
        };
        pending.extend_from_slice(&buf[..n]);
        let plain = match &sh.key {
            None => std::mem::take(&mut pending),
            Some(key) => {
                let whole = pending.len() / 16 * 16;
                if whole == 0 {
                    continue;
                }
                let chunk: Vec<u8> = pending.drain(..whole).collect();
                ElkCrypto::decrypt(key, &chunk).expect("key size")
            }
        };
        for f in parser.feed(&plain) {
            if f.kind != FrameKind::Message || !f.crc_ok {
                continue;
            }
            lock(&sh.received).push(f.payload.clone());
            let ok = match handle(sh, &f.payload) {
                Reply::Ack => write_raw(&mut net, sh, &[0x10, 0x06]),
                Reply::Msg(m) => send(&mut net, sh, &m),
                Reply::None => true,
            };
            if !ok {
                return;
            }
        }
    }
}

fn encode_out(sh: &Shared, data: &[u8]) -> Vec<u8> {
    match &sh.key {
        Some(k) => ElkCrypto::encrypt(k, data).expect("key size"),
        None => data.to_vec(),
    }
}

fn write_raw(net: &mut TcpStream, sh: &Shared, raw: &[u8]) -> bool {
    net.write_all(&encode_out(sh, raw)).is_ok()
}

fn send(net: &mut TcpStream, sh: &Shared, payload: &[u8]) -> bool {
    let enc = encode_out(sh, &Framing::encode(payload));
    if sh.split_writes.load(Ordering::SeqCst) && enc.len() > 5 {
        if net.write_all(&enc[..5]).is_err() {
            return false;
        }
        let _ = net.flush();
        thread::sleep(Duration::from_millis(30));
        net.write_all(&enc[5..]).is_ok()
    } else {
        net.write_all(&enc).is_ok()
    }
}

/// Lighting and voice messages are stored per item whatever the count byte is.
fn auto_part(section: u8, part: u8) -> u8 {
    if section == 2 {
        part
    } else {
        0
    }
}

fn auto_item_len(section: u8, part: u8) -> usize {
    match (section, part) {
        (2, 0) => 16,
        (2, 2) | (13, _) => 12,
        _ => 28,
    }
}

/// Automation sections 2, 13, 14 and 121 (reads) and their write opcodes 130, 141, 142, 249. `None` for any other request.
fn auto_reply(sh: &Shared, p: &[u8]) -> Option<Reply> {
    let item = p[2] as usize * 256 + p[3] as usize;
    match p[0] {
        2 | 13 | 14 => {
            let section = p[0];
            let n = if section == 2 { 1 } else { (p[1] as usize).max(1) };
            let len = auto_item_len(section, p[1]);
            let items = lock(&sh.auto_items);
            let mut r = p[..4].to_vec();
            for i in 0..n {
                let d = items.get(&(section, auto_part(section, p[1]), (item + i) as u16)).cloned().unwrap_or_else(|| vec![0; len]);
                r.extend_from_slice(&d);
            }
            Some(Reply::Msg(r))
        }
        130 | 141 | 142 => {
            let section = p[0] - 128;
            let n = if section == 2 { 1 } else { (p[1] as usize).max(1) };
            let len = auto_item_len(section, p[1]);
            if p.len() != 4 + n * len {
                return Some(Reply::None); // a malformed write is not answered
            }
            let mut items = lock(&sh.auto_items);
            for i in 0..n {
                items.insert((section, auto_part(section, p[1]), (item + i) as u16), p[4 + i * len..4 + (i + 1) * len].to_vec());
            }
            Some(Reply::Msg(p[..4].to_vec()))
        }
        121 => {
            let packets = lock(&sh.packets);
            let from = (item * 8).min(packets.len());
            let to = ((item + p[1] as usize) * 8).min(packets.len());
            let mut r = p[..4].to_vec();
            r.extend_from_slice(&packets[from..to]);
            Some(Reply::Msg(r))
        }
        249 => {
            let n = p[1] as usize;
            if p.len() != 4 + n * 8 || item + n > 528 {
                return Some(Reply::None);
            }
            lock(&sh.packets)[item * 8..(item + n) * 8].copy_from_slice(&p[4..]);
            Some(Reply::Msg(p[..4].to_vec()))
        }
        _ => None,
    }
}

fn record_key(section: u8, p: &[u8]) -> (u8, u8) {
    (section, if section == Section::WirelessZone as u8 { p[1].wrapping_add(1) } else { p[3] })
}

fn handle(sh: &Shared, p: &[u8]) -> Reply {
    if p.is_empty() {
        return Reply::None;
    }
    let l = p.len();
    if p[0] == 0x7F && l >= 4 && p[1] == 0 && p[3] == 1 {
        if l < 10 {
            return Reply::None;
        }
        let code: String = (0..6).map(|i| (b'0' + p[9 - i]) as char).collect(); // digits arrive last-first
        let mut r = vec![0u8; 25];
        r[0] = 0x7F;
        r[1] = if code == *lock(&sh.access_code) { 0 } else { 1 };
        r[4] = 5;
        r[5] = 2;
        r[6] = 9; // firmware 5.2.9
        r[8] = 0x31;
        r[9] = 0x41;
        r[10] = 0x59;
        r[11] = 0x26; // serial 31415926
        r[12] = 1;
        r[13] = 2;
        r[14] = 3;
        r[15] = 4;
        r[16] = 5;
        return Reply::Msg(r);
    }
    if p[0] == 0x7F && l == 4 && p[1] == 0 && p[2] == 0 && matches!(p[3], 6 | 7 | 8 | 16) {
        return Reply::Msg(live_reply(sh, p[3]));
    }
    if p[0] == 0x7F && l == 4 && p[1] == 0 && p[2] == 0 && p[3] == 3 {
        // read the clock
        let c = *lock(&sh.clock);
        return Reply::Msg(vec![
            0x7F,
            0,
            0,
            3,
            bcd(c.second() as i32),
            bcd(c.minute() as i32),
            bcd(c.hour() as i32),
            (c.weekday().num_days_from_sunday() + 1) as u8,
            bcd(c.day() as i32),
            bcd(c.month() as i32),
            bcd(c.year() - 2000),
        ]);
    }
    if p[0] == 0xFF && l == 11 && p[3] == 3 {
        // set the clock: BCD fields, then ACK
        if let Some(t) = NaiveDate::from_ymd_opt(unbcd(p[10]) + 2000, unbcd(p[9]) as u32, unbcd(p[8]) as u32)
            .and_then(|d| d.and_hms_opt(unbcd(p[6]) as u32, unbcd(p[5]) as u32, unbcd(p[4]) as u32))
        {
            *lock(&sh.clock) = t;
        }
        return Reply::Ack;
    }
    if p[0] == 0x7F && l == 10 && p[3] == 5 {
        // change the RP access code: digits last first, then ACK
        *lock(&sh.access_code) = (0..6).map(|i| (b'0' + p[9 - i]) as char).collect();
        return Reply::Ack;
    }
    if p[0] == 0x7B && l == 4 {
        // event log: 00 = start, 14 hi lo = next 20 entries
        if p[1] == 0 {
            return Reply::Msg(vec![0x7B, 0, 0, 0]);
        }
        let log = lock(&sh.log_records);
        let a = p[2] as usize * 256 + p[3] as usize;
        let start = if a == 65535 { 0 } else { a };
        let take = 20usize.min(log.len().saturating_sub(start));
        let mut r = vec![0x7B, take as u8, ((start + take) / 256) as u8, ((start + take) & 0xFF) as u8];
        for e in log.iter().skip(start).take(take) {
            r.extend_from_slice(e);
        }
        return Reply::Msg(r);
    }
    if p[0] == 0x7F && l == 6 && p[2] == 0x40 {
        lock(&sh.arm_requests).push(p.to_vec()); // arm / disarm: recorded, no reply
        return Reply::None;
    }
    if p[0] == 0x7F {
        return Reply::None; // keepalive / disconnect: no reply
    }
    if p[0] == 0x75 {
        return match lock(&sh.xep).handle(p) {
            Some(r) => Reply::Msg(r),
            None => Reply::None,
        };
    }
    if l < 4 {
        return Reply::None;
    }
    if let Some(r) = auto_reply(sh, p) {
        return r;
    }
    if p[0] == 0x7E {
        let key = record_key(p[1], p);
        let rec = match lock(&sh.records).get(&key) {
            Some(r) => r.clone(),
            None => return Reply::None,
        };
        let crc = crate::framing::Crc16::compute(&rec);
        return Reply::Msg(vec![0x7E, p[1], 0, p[3], (crc >> 8) as u8, (crc & 0xFF) as u8]);
    }
    if p[0] >= 128 {
        let section = p[0] - 128;
        let key = record_key(section, p);
        let mut recs = lock(&sh.records);
        if section == Section::Keypad as u8 {
            let mut full = recs.get(&key).cloned().unwrap_or_else(|| vec![0u8; 148]);
            let off = if p[1] == 0 { 0 } else { 28 };
            let data = &p[4..];
            if full.len() < off + data.len() {
                full.resize(off + data.len(), 0);
            }
            full[off..off + data.len()].copy_from_slice(data); // part 0 -> bytes 0..27, part 1 -> bytes 28..147
            recs.insert(key, full);
        } else {
            recs.insert(key, p[4..].to_vec());
        }
        return Reply::Msg(vec![p[0], p[1], p[2], p[3]]);
    }
    let key = record_key(p[0], p);
    let mut data = match lock(&sh.records).get(&key) {
        Some(d) => d.clone(),
        None => return Reply::None,
    };
    if p[0] == Section::Keypad as u8 {
        data = if p[1] == 0 { data[..28.min(data.len())].to_vec() } else { data[28.min(data.len())..].to_vec() };
    }
    let mut r = vec![p[0], p[1], 0, p[3]];
    r.extend_from_slice(&data);
    Reply::Msg(r)
}

/// Canned status replies shaped like the real panel's: area 1 not ready, areas 2-8 ready, area 3 armed away; zones 1-16 open/violated;
/// A.C. failure as the only system trouble.
fn live_reply(sh: &Shared, n: u8) -> Vec<u8> {
    let mut r = vec![
        0u8;
        if n == 6 {
            68
        } else if n == 7 {
            56
        } else {
            212
        }
    ];
    r[0] = 0x7F;
    r[3] = n;
    if n == 6 {
        for a in 2..=8 {
            r[8 * a] = 0x04;
        }
        r[8 * 3] = 0x24;
        for (a, v) in lock(&sh.area_status).iter() {
            r[8 * *a as usize] = *v;
        }
    }
    if n == 8 {
        for z in 1..=16 {
            r[z + 3] = 0x09;
        }
    }
    if n == 16 {
        for z in 1..=16 {
            r[z + 3] = 0x89;
        }
    }
    if n == 7 {
        r[4 + 18] = 1;
    }
    r
}
