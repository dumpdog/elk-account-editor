//! Port of `Elk.Cli/Program.cs`: the `elk` command line tool. `run` takes the arguments and the two output streams, so the commands
//! can be tested without a process. Errors that the C# lets crash the program (a missing option, a bad number) print `error: ...`
//! on stderr and return 1 here.

use std::io::Write;
use std::time::Duration;

use elk_account::{
    decode_latin1, encode_latin1, AccountExport, AccountFile, AccountModel, AlarmZoneList, ZoneCsv,
};
use elk_core::{
    hex_decode, hex_upper, AreaRecord, Crc16, ElkError, ElkSession, Framing, KeypadRecord,
    PanelBackup, PanelControl, PanelFile, PanelLive, PanelModules, ZoneFlag2, ZoneRecord,
};
use elk_panel::sync::{
    PanelConnection, PanelSync, Settings, DEFAULT_ATTEMPTS, DEFAULT_RETRY_DELAY,
};

/// Failure of one command: the text for stderr.
type Fail = String;
type Res<T> = Result<T, Fail>;

fn e(err: impl std::fmt::Display) -> Fail {
    err.to_string()
}

fn hex(s: &str) -> Res<Vec<u8>> {
    hex_decode(&s.replace(' ', "")).map_err(e)
}

/// The value after option `n`, if there is one.
fn opt<'a>(a: &'a [String], n: &str) -> Option<&'a str> {
    let i = a.iter().position(|x| x == n)?;
    a.get(i + 1).map(String::as_str)
}

fn need<'a>(a: &'a [String], n: &str) -> Res<&'a str> {
    opt(a, n).ok_or_else(|| format!("missing {n}"))
}

fn has(a: &[String], n: &str) -> bool {
    a.iter().any(|x| x == n)
}

fn num<T: std::str::FromStr>(s: &str) -> Res<T> {
    s.trim()
        .parse()
        .map_err(|_| format!("The input string '{s}' was not in a correct format."))
}

/// `File.ReadAllText(path, Encoding.Latin1)`.
fn read_latin1(path: &str) -> Res<String> {
    std::fs::read(path).map(|b| decode_latin1(&b)).map_err(e)
}

/// `File.ReadAllText(path)`: UTF-8, a byte order mark is dropped.
fn read_utf8(path: &str) -> Res<String> {
    let b = std::fs::read(path).map_err(e)?;
    let b = b.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(&b);
    Ok(String::from_utf8_lossy(b).into_owned())
}

/// .NET `ToString("0.0")` of a double (exact ties round away from zero, which Rust's `{:.1}` does not).
pub fn fmt1(v: f64) -> String {
    let q = v * 4.0;
    if q.fract() == 0.0 && (q.abs() as i64) % 2 == 1 {
        let r = (v * 10.0).round() / 10.0;
        return format!("{r:.1}");
    }
    format!("{v:.1}")
}

/// Runs one command line (without the program name) and returns the exit code.
pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if args.is_empty() {
        let _ = writeln!(out, "usage: elk crc|frame|decode|download|upload ...");
        return 1;
    }
    match command(args, out, err) {
        Ok(code) => code,
        Err(m) => {
            let _ = writeln!(err, "error: {m}");
            1
        }
    }
}

fn command(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> Res<i32> {
    macro_rules! say {
        ($($t:tt)*) => { writeln!(out, $($t)*).map_err(e)? };
    }
    macro_rules! warn_ {
        ($($t:tt)*) => { writeln!(err, $($t)*).map_err(e)? };
    }
    match args[0].as_str() {
        "crc" => {
            say!(
                "{:04X}",
                Crc16::compute(&hex(args.get(1).ok_or("missing hex")?)?)
            );
            Ok(0)
        }
        "frame" => {
            say!(
                "{}",
                hex_upper(&Framing::encode(&hex(args.get(1).ok_or("missing hex")?)?))
            );
            Ok(0)
        }
        "decode" => {
            let kind = args.get(1).ok_or("missing record kind")?;
            let b = hex(args.get(2).ok_or("missing hex")?)?;
            match kind.as_str() {
                "zone" => {
                    let z = ZoneRecord::from_bytes(&b);
                    say!(
                        "name='{}' function={} flag1={} flag2={} (type {}, area {}) crc={}",
                        z.name(),
                        z.function(),
                        z.flag1(),
                        z.flag2(),
                        ZoneFlag2::type_of(z.flag2()),
                        ZoneFlag2::area(z.flag2()),
                        z.crc()
                    );
                }
                "area" => {
                    if b.len() > AreaRecord::SIZE {
                        return Err("Destination is too short.".into());
                    }
                    let mut a = AreaRecord::new();
                    a.bytes[..b.len()].copy_from_slice(&b);
                    say!(
                        "name='{}' exit1={} entrance1={} exit2={} entrance2={} flags1={} flags2={} armWindow={} crc={}",
                        a.name(),
                        a.exit1(),
                        a.entrance1(),
                        a.exit2(),
                        a.entrance2(),
                        a.flags1(),
                        a.flags2(),
                        a.arm_time_window(),
                        a.crc()
                    );
                }
                "keypad" => {
                    if b.len() > KeypadRecord::SIZE {
                        return Err("Destination is too short.".into());
                    }
                    let mut k = KeypadRecord::new();
                    k.bytes[..b.len()].copy_from_slice(&b);
                    say!(
                        "name='{}' area={} crc1={} crc2={}",
                        k.name(),
                        k.area(),
                        k.crc1(),
                        k.crc2()
                    );
                }
                _ => {
                    warn_!("zone|area|keypad");
                    return Ok(1);
                }
            }
            Ok(0)
        }
        "inspect" => {
            let f = AccountFile::parse(&read_latin1(need(args, "--in")?)?).map_err(e)?;
            say!(
                "account '{}' {} sw {}: {} users, {} areas, {} keypads, {} zones, {} phones",
                f.account_id(),
                f.panel_type(),
                f.software_version(),
                f.users.entries.len(),
                f.areas.entries.len(),
                f.keypads.entries.len(),
                f.zones.entries.len(),
                f.phones.entries.len()
            );
            say!("zone groups: {}", f.zones.trailer.join(","));
            let take = if has(args, "--all") { 999 } else { 4 };
            for (name, s) in [
                ("user", &f.users),
                ("area", &f.areas),
                ("keypad", &f.keypads),
                ("phone", &f.phones),
            ] {
                for en in s.entries.iter().take(take) {
                    say!("{} {}: {}", name, en.number, hex_upper(&en.bytes));
                }
            }
            Ok(0)
        }
        "export-zones" => {
            let (specs, errors) = ZoneCsv::parse(&read_utf8(need(args, "--csv")?)?);
            for m in &errors {
                warn_!("CSV error: {m}");
            }
            if !errors.is_empty() || specs.is_empty() {
                warn_!("Fix the CSV first; nothing was written.");
                return Ok(2);
            }
            let in_path = need(args, "--in")?;
            let out_path = need(args, "--out")?;
            if std::path::absolute(in_path).map_err(e)?
                == std::path::absolute(out_path).map_err(e)?
            {
                warn_!("--out must be a different file from --in.");
                return Ok(2);
            }
            match AccountExport::apply_zones(&read_latin1(in_path)?, &specs) {
                Ok((text, log)) => {
                    for l in &log {
                        say!("{l}");
                    }
                    std::fs::write(out_path, encode_latin1(&text)).map_err(e)?;
                    say!("wrote {out_path}. In ElkRP use File > Import Account.");
                    Ok(0)
                }
                Err(elk_account::Error::InvalidData(m)) => {
                    warn_!("{m}");
                    Ok(3)
                }
                Err(x) => Err(e(x)),
            }
        }
        "export-alarm-list" => {
            let model = AccountModel::from(
                &AccountFile::parse(&read_latin1(need(args, "--in")?)?).map_err(e)?,
            )
            .map_err(e)?;
            let out_path = need(args, "--out")?;
            let bytes = AlarmZoneList::build(&model, opt(args, "--title"), None).map_err(e)?;
            std::fs::write(out_path, bytes).map_err(e)?;
            say!("wrote {out_path}");
            Ok(0)
        }
        _ => panel_command(args, out, err),
    }
}

fn open(a: &[String], err: &mut dyn Write) -> Res<ElkSession> {
    let plain = has(a, "--plain");
    let port: i32 = num(opt(a, "--port").unwrap_or(if plain { "2101" } else { "2601" }))?;
    let settings = Settings {
        host: need(a, "--host")?.to_string(),
        port,
        code: need(a, "--code")?.to_string(),
        plain,
        mac: opt(a, "--mac").map(str::to_string),
        passphrase: opt(a, "--pass").map(str::to_string),
        tls: has(a, "--tls"),
        serial: opt(a, "--serial").map(str::to_string),
    };
    let mut log = |m: &str| {
        let _ = writeln!(err, "{m}");
    };
    PanelConnection::open(&settings, &mut log, DEFAULT_ATTEMPTS, DEFAULT_RETRY_DELAY).map_err(e)
}

fn disconnect(s: &mut ElkSession) -> Res<()> {
    s.disconnect().map_err(e)
}

fn module_lines(s: &mut ElkSession, out: &mut dyn Write) -> Res<()> {
    for m in PanelModules::scan(s).map_err(e)? {
        writeln!(
            out,
            "{:<22} {:>2}  {:<10} hardware {:<6} boot {:<8} firmware {}",
            m.kind, m.address, m.model, m.hardware, m.boot, m.firmware
        )
        .map_err(e)?;
    }
    Ok(())
}

fn area_line(a: &elk_core::AreaLive) -> String {
    format!(
        "area {}: {}, {}{}",
        a.area,
        if a.ready { "ready" } else { "not ready" },
        a.armed_name,
        if a.alarm { ", ALARM" } else { "" }
    )
}

fn panel_command(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> Res<i32> {
    macro_rules! say {
        ($($t:tt)*) => { writeln!(out, $($t)*).map_err(e)? };
    }
    match args[0].as_str() {
        "download" => {
            let mut s = open(args, err)?;
            let with_auto = has(args, "--automation");
            let f = if with_auto {
                PanelBackup::read_with_automation(&mut s)
            } else {
                PanelBackup::read(&mut s)
            }
            .map_err(e)?;
            disconnect(&mut s)?;
            std::fs::write(need(args, "--out")?, f.to_json()).map_err(e)?;
            say!(
                "saved {} zones, {} areas, {} keypads, {} other blocks",
                f.zones.len(),
                f.areas.len(),
                f.keypads.len(),
                f.blocks.len()
            );
            if with_auto {
                say!("and {} automation blocks (outputs, tasks, counters, custom values, lights, voice messages, rule and text packets)", f.automation.len());
            }
            Ok(0)
        }
        "upload" => {
            let f = PanelFile::from_json(&read_utf8(need(args, "--in")?)?).map_err(e)?;
            let apply = has(args, "--yes");
            let mut s = open(args, err)?;
            let mut lines: Vec<String> = Vec::new();
            let changed = PanelBackup::update(
                &mut s,
                &f,
                apply,
                Some(&mut |m: &str| lines.push(m.to_string())),
            )
            .map_err(e)?;
            for l in &lines {
                say!("{l}");
            }
            disconnect(&mut s)?;
            if apply {
                say!("wrote {changed} record(s)");
            } else {
                say!("{changed} record(s) would change. Re-run with --yes to write them.");
            }
            Ok(0)
        }
        "upload-export" => {
            let f = AccountFile::parse(&read_latin1(need(args, "--in")?)?).map_err(e)?;
            let apply = has(args, "--yes");
            let mut s = open(args, err)?;
            let r = PanelSync::diff_with(
                &mut s,
                &f,
                has(args, "--reset-missing"),
                has(args, "--send-local-code"),
                has(args, "--automation"),
            )
            .map_err(e)?;
            for c in &r.changes {
                say!("{} {} differs", c.label, c.number);
            }
            if apply {
                PanelSync::apply(&mut s, &r.changes, None).map_err(e)?;
            }
            disconnect(&mut s)?;
            for (label, (same, differ)) in &r.counts {
                say!(
                    "{:<22} {} already match, {} {}",
                    label,
                    same,
                    differ,
                    if apply { "written" } else { "would change" }
                );
            }
            if !r.skipped.is_empty() {
                say!("not on the panel (skipped): {}", r.skipped.join(", "));
            }
            if !apply {
                say!("dry run; add --yes to write");
            }
            Ok(0)
        }
        "modules" => {
            let mut s = open(args, err)?;
            module_lines(&mut s, out)?;
            disconnect(&mut s)?;
            Ok(0)
        }
        "enroll" => {
            if !has(args, "--yes") {
                say!("This makes the panel rescan its data bus and re-enroll the modules it finds. Add --yes to do it.");
                return Ok(0);
            }
            {
                let mut s0 = open(args, err)?;
                say!("{}", PanelModules::enroll(&mut s0).map_err(e)?);
                match s0.read_block(7, 1, 0) {
                    Ok(_) => say!("the connection is still open after enrolling"),
                    Err(x) => say!(
                        "the connection ended after enrolling ({})",
                        exception_name(&x)
                    ),
                }
            }
            say!("Enrolling... waiting 15 seconds");
            std::thread::sleep(Duration::from_secs(15));
            let mut s = open(args, err)?;
            module_lines(&mut s, out)?;
            disconnect(&mut s)?;
            Ok(0)
        }
        "status" => {
            let mut s = open(args, err)?;
            let st = PanelLive::read(&mut s).map_err(e)?;
            for a in &st.areas {
                say!("{}", area_line(a));
            }
            for z in st
                .zones
                .iter()
                .filter(|z| z.state != "Normal" || z.bypassed || z.force_armed)
                .take(40)
            {
                say!(
                    "zone {}: {}{}, {}, {} V",
                    z.zone,
                    z.state,
                    if z.bypassed { ", bypassed" } else { "" },
                    z.physical,
                    fmt1(z.volts)
                );
            }
            say!(
                "system: {}",
                if st.system.is_empty() {
                    "no alarms or troubles".to_string()
                } else {
                    st.system.join(", ")
                }
            );
            disconnect(&mut s)?;
            Ok(0)
        }
        "control" => {
            let what = need(args, "--do")?;
            let area: i32 = num(need(args, "--area")?)?;
            let mut s = open(args, err)?;
            let reply = match what {
                "bypass" => PanelControl::toggle_bypass(&mut s, num(need(args, "--zone")?)?, area),
                "arm" => PanelControl::arm_checked(
                    &mut s,
                    area,
                    num(need(args, "--level")?)?,
                    has(args, "--force"),
                ),
                "silence" => PanelControl::silence(&mut s, area),
                _ => return Err("--do must be bypass, arm or silence".into()),
            }
            .map_err(e)?;
            say!(
                "reply: {}",
                reply.map_or("(none)".to_string(), |r| hex_upper(&r))
            );
            std::thread::sleep(Duration::from_millis(800));
            let st = PanelLive::read(&mut s).map_err(e)?;
            for a in st.areas.iter().filter(|a| a.area == area) {
                say!("{}", area_line(a));
            }
            if has(args, "--zone") {
                let zn: usize = num(need(args, "--zone")?)?;
                let z = zn.checked_sub(1).and_then(|i| st.zones.get(i)).ok_or("Index was out of range. Must be non-negative and less than the size of the collection.")?;
                say!(
                    "zone {}: {}{}, {}",
                    z.zone,
                    z.state,
                    if z.bypassed { ", bypassed" } else { "" },
                    z.physical
                );
            }
            disconnect(&mut s)?;
            Ok(0)
        }
        "raw-read" => {
            let mut s = open(args, err)?;
            let sec: u8 = num(need(args, "--section")?)?;
            let item: u8 = num(need(args, "--item")?)?;
            let part: u8 = num(opt(args, "--part").unwrap_or("0"))?;
            s.send(&[sec, part, 0, item]).map_err(e)?;
            let r = s.receive(sec).map_err(e)?;
            say!("{} bytes: {}", r.len(), hex_upper(&r));
            disconnect(&mut s)?;
            Ok(0)
        }
        "raw-write" => {
            let sec: u8 = num(need(args, "--section")?)?;
            let item: u8 = num(need(args, "--item")?)?;
            let part: u8 = num(opt(args, "--part").unwrap_or("0"))?;
            let data = hex(need(args, "--data")?)?;
            say!(
                "would write section {} item {} part {}: {} bytes {}",
                sec,
                item,
                part,
                data.len(),
                hex_upper(&data)
            );
            if !has(args, "--yes") {
                say!("dry run; add --yes to send");
                return Ok(0);
            }
            let mut s = open(args, err)?;
            s.write_record(sec, item, &data, part).map_err(e)?;
            disconnect(&mut s)?;
            say!("written");
            Ok(0)
        }
        _ => {
            writeln!(err, "unknown command").map_err(e)?;
            Ok(1)
        }
    }
}

/// The .NET exception class name for the `enroll` message.
fn exception_name(x: &ElkError) -> &'static str {
    match x {
        ElkError::Io(_) => "IOException",
        ElkError::Timeout(_) => "TimeoutException",
        ElkError::InvalidData(_) => "InvalidDataException",
        ElkError::Unauthorized(_) => "UnauthorizedAccessException",
        ElkError::InvalidOperation(_) => "InvalidOperationException",
        ElkError::Argument(_) => "ArgumentException",
        ElkError::OutOfRange(_) => "ArgumentOutOfRangeException",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volts_round_half_away_from_zero() {
        assert_eq!(fmt1(2.25), "2.3");
        assert_eq!(fmt1(2.75), "2.8");
        assert_eq!(fmt1(0.0), "0.0");
        assert_eq!(fmt1(5.04), "5.0");
        assert_eq!(fmt1(12.6), "12.6");
    }

    #[test]
    fn option_lookup() {
        let a: Vec<String> = ["x", "--in", "f", "--yes"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(opt(&a, "--in"), Some("f"));
        assert_eq!(opt(&a, "--yes"), None);
        assert!(has(&a, "--yes"));
        assert!(need(&a, "--out").is_err());
    }
}
