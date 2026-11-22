//! ElkRP's English wording of rules (port of `WhenSelect.ConstructEnglishTrigger / ConstructEnglishCondition /
//! ConstructEnglishResult`, `DoZoneEnglish`, `GetName` and the `Get*Name` functions of ElkRP 5.3, M1G, ILSpy output under
//! `Elk\src\M1G2\M1G`), plus the grouping `WhenThen.PopulateRules` does (WHENEVER, AND ..., THEN ..., FOR).
//! Names come from the account: zones, areas, keypads, users, telephone numbers, tasks, outputs, counters, custom values,
//! thermostats, lights and text strings. Items ElkRP reads from other places (e-mail names, audio zone / source names, which
//! live in the M1XEP configuration, not in the export file) use ElkRP's own text for an item without a name ("Email 3").

use std::collections::HashMap;

use crate::account_file::AccountFile;
use crate::automation::{
    name_at, text_starts, text_strings, AutomationBlock, AutomationMeta, PACKET_COUNT,
};
use crate::records::{ZoneFlag2, ZoneRecord};
use crate::rules::{event_names, group};

/// Names that rules refer to and that do not live in the automation block.
#[derive(Debug, Clone, Default)]
pub struct Names {
    /// zone number to (name, zone type 0-6)
    pub zones: HashMap<i32, (String, i32)>,
    pub areas: HashMap<i32, String>,
    pub keypads: HashMap<i32, String>,
    pub users: HashMap<i32, String>,
    /// telephone number to (name, number)
    pub phones: HashMap<i32, (String, String)>,
}

fn trim_field(b: &[u8], at: usize, len: usize) -> String {
    let s: String = b[at..at + len].iter().map(|&c| c as char).collect();
    s.trim_end_matches(['\0', ' ']).to_string()
}

impl Names {
    pub fn from_file(f: &AccountFile) -> Names {
        let mut n = Names::default();
        for e in &f.zones.entries {
            let z = ZoneRecord::from_bytes(&e.bytes);
            n.zones
                .insert(e.number, (z.name(), ZoneFlag2::type_of(z.flag2())));
        }
        for e in &f.areas.entries {
            n.areas.insert(e.number, trim_field(&e.bytes, 0, 16));
        }
        for e in &f.keypads.entries {
            n.keypads.insert(e.number, trim_field(&e.bytes, 6, 16));
        }
        for e in &f.users.entries {
            n.users.insert(e.number, trim_field(&e.bytes, 9, 16));
        }
        for e in &f.phones.entries {
            n.phones.insert(
                e.number,
                (trim_field(&e.bytes, 71, 16), trim_field(&e.bytes, 0, 20)),
            );
        }
        n
    }
}

/// Everything the formatter reads.
pub struct Ctx<'a> {
    pub a: &'a AutomationBlock,
    pub n: &'a Names,
}

fn ptype(p: &[u8; 8]) -> u8 {
    p[0] / 8
}
fn rtype(p: &[u8; 8]) -> u8 {
    p[4] / 8
}

/// .NET `Math.Round(double)`: ties to even.
fn bank_round(x: f64) -> i32 {
    let r = x.round();
    if (x - x.trunc()).abs() == 0.5 && (r as i64) % 2 != 0 {
        (r - x.signum()) as i32
    } else {
        r as i32
    }
}

fn f_to_c(f: i32) -> i32 {
    bank_round(((f - 32) * 5) as f64 / 9.0)
}

pub const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

fn letter16(i: usize) -> char {
    (b'A' + (i % 16) as u8) as char
}

impl Ctx<'_> {
    // ---- names ----------------------------------------------------------------------------------------------------
    pub fn zone_name(&self, n: i32) -> String {
        match self.n.zones.get(&n) {
            Some((name, _)) => format!("{} (Zn {n})", name.trim()),
            None => format!("INVALID (Zn {n})"),
        }
    }
    fn zone_hookup(&self, n: i32) -> i32 {
        self.n.zones.get(&n).map_or(0, |z| z.1)
    }
    pub fn area_name(&self, n: i32) -> String {
        match self.n.areas.get(&n) {
            Some(name) => format!("{name} (Area {n})"),
            None => format!("Area {n} (invalid)"),
        }
    }
    pub fn keypad_name(&self, n: i32) -> String {
        match self.n.keypads.get(&n) {
            Some(name) => format!("{name} (KP {n})"),
            None => format!("KP {n} (invalid)"),
        }
    }
    pub fn user_name(&self, n: i32) -> String {
        match n {
            201 => "\"Installer\" (User 201)".to_string(),
            202 => "\"ElkRP\" (User 202)".to_string(),
            _ => match self.n.users.get(&n) {
                Some(name) => format!("{name} (User {n})"),
                None => format!("User {n}"),
            },
        }
    }
    fn tele_name(&self, n: i32) -> String {
        match self.n.phones.get(&n) {
            Some((name, num)) => format!("{name} ({num})"),
            None => format!("Telephone {n} (invalid)"),
        }
    }
    pub fn task_name(&self, n: i32) -> String {
        if !(0..=32).contains(&n) || n == 0 {
            return String::new();
        }
        let t = name_at(&self.a.tasks[n as usize - 1], 0, 16);
        if t.is_empty() {
            format!("Task {n}")
        } else {
            format!("{t} (Task {n})")
        }
    }
    pub fn cust_name(&self, n: i32) -> String {
        if !(1..=32).contains(&n) {
            return String::new();
        }
        let t = name_at(&self.a.customs[n as usize - 1], 4, 12);
        let kind = if n <= 12 {
            format!("CO Timer {n}")
        } else {
            format!("Cust Set {}", n - 12)
        };
        if t.is_empty() {
            kind
        } else {
            format!("{t} ({kind})")
        }
    }
    pub fn cntr_name(&self, n: i32) -> String {
        if !(1..=64).contains(&n) {
            return String::new();
        }
        let t = name_at(&self.a.counters[n as usize - 1], 0, 12);
        if t.is_empty() {
            format!("Counter {n}")
        } else {
            format!("{t} (Counter {n})")
        }
    }
    pub fn tstat_name(&self, n: i32) -> String {
        if !(1..=16).contains(&n) {
            return String::new();
        }
        let t = name_at(&self.a.counters[64 + n as usize - 1], 0, 12);
        if t.is_empty() {
            format!("Tstat {n}")
        } else {
            format!("{t} (Tstat {n})")
        }
    }
    pub fn output_name(&self, n: i32) -> String {
        if !(1..=64).contains(&n) {
            return format!("Output {n}");
        }
        let t = name_at(&self.a.outputs[n as usize - 1], 0, 16);
        if t.is_empty() {
            format!("Output {n}")
        } else {
            format!("{t} (Out {n})")
        }
    }
    /// `item` is the 1-based lighting number (ElkRP passes `Packet + 1`).
    pub fn light_name(&self, item: i32) -> String {
        if !(1..=256).contains(&item) {
            return String::new();
        }
        let l = &self.a.lights[item as usize - 1];
        let t = name_at(l, 1, 15);
        let dev = format!(
            "{item} ({}{})",
            letter16(((item - 1) / 16) as usize),
            ((item - 1) & 15) + 1
        );
        if t.is_empty() {
            format!("Lighting {dev}")
        } else {
            format!("{t} [{dev}]")
        }
    }
    fn light_format(&self, item: i32) -> u8 {
        self.a
            .lights
            .get((item - 1).max(0) as usize)
            .map_or(0, |l| (l[0] / 32) & 7)
    }
    fn light_type(&self, item: i32) -> u8 {
        self.a
            .lights
            .get((item - 1).max(0) as usize)
            .map_or(0, |l| (l[0] / 4) & 7)
    }
    fn vm_name(&self, n: i32) -> String {
        match AutomationMeta::voice_message_names().get(n) {
            Some(s) => s.rsplit('|').next().unwrap_or("").to_string(),
            None => String::new(),
        }
    }

    // ---- text strings ---------------------------------------------------------------------------------------------
    /// `PrintableText(Bytes2Text(first packet))`
    fn text_at(&self, packet: usize) -> String {
        let first = PACKET_COUNT - self.a.text_packets_used;
        if packet < first || packet >= PACKET_COUNT {
            return "?".to_string();
        }
        let starts = text_starts(self.a);
        let strs = text_strings(self.a);
        let raw = starts
            .iter()
            .position(|&s| s == packet)
            .and_then(|i| strs.get(i).cloned())
            .unwrap_or_default();
        printable(&raw)
    }

    // ---- event names ----------------------------------------------------------------------------------------------
    /// `GetName`: the numbered items inside an event description get the account's names.
    fn get_name(&self, s: String) -> String {
        let s = self.sub(s, "ZONE ", |n| self.zone_name(n));
        let s = self.sub(s, "AREA ", |n| self.area_name(n));
        let s = self.sub(s, "KEYPAD ", |n| self.keypad_name(n));
        let s = self.sub(s, "OUTPUT ", |n| self.output_name(n));
        self.sub(s, "AUTOMATION TASK ", |n| self.task_name(n))
    }
    fn sub(&self, s: String, key: &str, f: impl Fn(i32) -> String) -> String {
        let Some(at) = s.find(key) else { return s };
        let rest = &s[at + key.len()..];
        // VB Val: leading digits
        let digits: String = rest
            .trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        let n: i32 = digits.parse().unwrap_or(0);
        if n <= 0 {
            return s;
        }
        let tail = match rest.find(' ') {
            Some(p) => &rest[p + 1..],
            None => "",
        };
        format!("{}{} {}", &s[..at], f(n), tail)
    }

    fn event_parts(&self, ev: i32) -> Option<[String; 9]> {
        let line = event_row(ev)?;
        Some(line)
    }

    // ---- trigger --------------------------------------------------------------------------------------------------
    pub fn trigger(&self, p: &[u8; 8]) -> String {
        let t = ptype(p);
        let mut s = String::from("WHENEVER ");
        match t {
            0 => return String::new(),
            2 => {
                if p[0] & 7 == 7 {
                    s += "EVERY ";
                    if p[2] > 0 {
                        s += &format!("{} HOUR{}", p[2], if p[2] == 1 { " " } else { "S " });
                        if p[3] > 0 {
                            s += "AND ";
                        }
                    }
                    if p[3] > 0 {
                        s += &format!("{} MINUTE{}", p[3], if p[3] == 1 { " " } else { "S " });
                    }
                } else {
                    s += "THE TIME IS ";
                    if p[1] > 0 {
                        s += &self.cust_name((p[1] & 0x7F) as i32);
                    } else {
                        s += &clock12(p[2], p[3]);
                    }
                }
            }
            3 => {
                s += "THE HOUR IS ";
                if p[2] & 0x80 != 0 {
                    s += &self.cust_name((p[2] & 0x7F) as i32);
                } else {
                    let h = if p[1] > 12 { p[1] - 12 } else { p[1] };
                    s += &format!("{h}:xx {}", if p[1] >= 12 { " PM" } else { " AM" });
                }
                s += "     (Note: This event triggers every minute for the whole hour specified.)";
            }
            4 => {
                s += "THE MINUTE IS xx:";
                if p[2] & 0x80 != 0 {
                    s += &self.cust_name((p[2] & 0x7F) as i32);
                } else {
                    s += &format!("{:02}", p[1]);
                }
            }
            9 | 10 => s += &sun(p, true),
            11 => {
                let ev = p[2] as i32 * 256 + p[3] as i32;
                let pro = p[1] != 0;
                let Some(row) = self.event_parts(ev) else {
                    return format!("{s}{}", unknown_event(ev));
                };
                let col = if pro { 0 } else { 1 }; // pre: [1],[2]; post: [3],[4]
                let name = self.get_name(row[0].chars().skip(7).collect());
                let pre = &row[1 + col];
                let post = &row[3 + col];
                if pre == "***" {
                    s += post;
                } else {
                    if !pre.is_empty() {
                        s += pre;
                        s += " ";
                    }
                    s += &name;
                    if !post.is_empty() {
                        s += " ";
                        s += post;
                    }
                }
            }
            12 => {
                s += &self.zone_name(p[3] as i32);
                s += " BECOMES ";
                self.zone_english(p, &mut s);
            }
            13 => {
                s += &format!(
                    "THE FOLLOWING TEXT IS RECEIVED: \"{}\"",
                    self.text_at(p[2] as usize * 256 + p[3] as usize)
                );
                s += &format!(" THROUGH PORT {}", p[1]);
            }
            14 => s += &self.lighting_trigger(p),
            16 | 23 => {
                s += &self.cntr_name(p[1] as i32);
                s += " CHANGES TO ";
                s += match p[0] & 7 {
                    2 => "ANY VALUE OTHER THAN ",
                    4 => "A VALUE LESS THAN ",
                    3 => "A VALUE GREATER THAN ",
                    _ => "",
                };
                if t == 16 {
                    s += &(p[2] as i32 * 256 + p[3] as i32).to_string();
                } else {
                    s += &self.cust_name(p[3] as i32);
                }
            }
            20 => {
                s += "EVERY ";
                if p[0] & 1 != 0 {
                    s += &self.cust_name((p[1] & 0x7F) as i32);
                } else {
                    s += &(p[2] as i32 * 256 + p[3] as i32).to_string();
                }
                s += " SECONDS";
            }
            21 => {
                let mut t2 = format!("{} ", self.tstat_name(p[1] as i32));
                match p[3] & 7 {
                    0 => {
                        t2 += "IS SET TO ";
                        t2 += match p[2] {
                            0 => "OFF",
                            1 => "HEAT",
                            2 => "COOL",
                            3 => "AUTO",
                            _ => "",
                        };
                    }
                    1 => {
                        t2 += "HOLD IS SET TO ";
                        t2 += if p[2] > 0 { "ON" } else { "OFF" };
                    }
                    2 => {
                        t2 += "FAN IS SET TO ";
                        t2 += if p[2] > 0 { "ON" } else { "AUTO" };
                    }
                    k @ (4 | 5) => {
                        t2 += if k == 4 {
                            "COOLING DESIRED TEMP IS SET TO "
                        } else {
                            "HEATING DESIRED TEMP IS SET TO "
                        };
                        t2 += match p[0] & 7 {
                            2 => "ANY VALUE OTHER THAN ",
                            3 => "GREATER THAN ",
                            4 => "LESS THAN ",
                            _ => "",
                        };
                        t2 += &self.temp_operand(p[3] & 0x80 != 0, p[3] & 0x40 != 0, p[2]);
                    }
                    _ => {}
                }
                s += &t2;
            }
            1 | 5 | 6 | 7 | 8 | 15 | 18 | 19 => {}
            _ => return String::new(),
        }
        s
    }

    fn temp_operand(&self, custom: bool, counter: bool, v: u8) -> String {
        if custom {
            self.cust_name(v as i32)
        } else if counter {
            self.cntr_name(v as i32)
        } else {
            format!("{} DEG. F. ({} DEG. C.)", v, f_to_c(v as i32))
        }
    }

    fn lighting_trigger(&self, p: &[u8; 8]) -> String {
        if p[3] & 0xF0 == 0 {
            let item = p[1] as i32 + 1;
            let mut s = format!("{} ", self.light_name(item));
            let fmt = self.light_format(item);
            let ty = self.light_type(item);
            let tmp: String = if fmt != 3 && ty == 2 {
                if p[2] == 0 {
                    (if p[0] & 7 == 3 {
                        "IS TURNED ON"
                    } else {
                        "IS TURNED OFF"
                    })
                    .to_string()
                } else {
                    let mut t = String::from("IS SET ");
                    t += match p[0] & 7 {
                        1 => "TO LEVEL ",
                        2 => "TO ANY LEVEL OTHER THAN ",
                        4 => "TO A LEVEL LESS THAN ",
                        3 => "TO A LEVEL GREATER THAN ",
                        _ => "",
                    };
                    t += &format!("{}%", p[2]);
                    t
                }
            } else if fmt == 3 && ty == 2 {
                if p[2] < 2 {
                    (if p[2] == 1 || p[0] & 7 == 3 {
                        "IS TURNED ON"
                    } else {
                        "IS TURNED OFF"
                    })
                    .to_string()
                } else {
                    format!("CHANGES TO SCENE {}", choose(&SCENES, p[2] as usize))
                }
            } else if p[2] > 0 || p[0] & 7 == 3 {
                "IS TURNED ON".to_string()
            } else {
                "IS TURNED OFF".to_string()
            };
            let mut tmp = tmp;
            if p[3] & 1 != 0 {
                tmp += " BY THE M1";
            } else if p[3] & 2 != 0 {
                tmp += " BY SOME EXTERNAL DEVICE";
            }
            s += &tmp;
            s
        } else {
            let mut t = String::from(if (p[3] & 0xF0) / 16 == 2 {
                "\"ALL LIGHTS ON\" FOR HOUSECODE "
            } else {
                "\"ALL UNITS OFF\" FOR HOUSECODE "
            });
            t.push_str(&choose(&HOUSES, (p[1] / 16) as usize + 1));
            t += if p[3] & 1 != 0 {
                " IS SENT BY THE M1"
            } else if p[3] & 2 != 0 {
                " IS SENT BY SOME EXTERNAL DEVICE"
            } else {
                " IS SENT OR DETECTED"
            };
            t
        }
    }

    fn zone_english(&self, p: &[u8; 8], s: &mut String) {
        if p[2] & 0x80 == 0x80 {
            let pre = match p[0] & 7 {
                1 => "EQUAL TO ANALOG VALUE ",
                2 => "NOT EQUAL TO ANALOG VALUE ",
                4 => "LESS THAN ANALOG VALUE ",
                3 => "GREATER THAN ANALOG VALUE ",
                _ => "",
            };
            let v = (p[1] as f64 * 138.0) / 2550.0;
            *s = format!("{s}{pre}{:.1} VOLTS", (v * 10.0).round() / 10.0);
        } else {
            let hook = self.zone_hookup(p[3] as i32);
            *s += match p[1] {
                0 => {
                    if hook >= 5 {
                        "NORMAL"
                    } else {
                        "SECURE"
                    }
                }
                1 => "TROUBLED",
                2 => {
                    if hook >= 5 {
                        "VIOLATED"
                    } else {
                        "NOT SECURE"
                    }
                }
                3 => "NOT SECURE - OPEN LOOP",
                4 => "NORMAL",
                5 => "NOT SECURE - SHORTED",
                _ => "",
            };
        }
    }

    // ---- condition (AND) ------------------------------------------------------------------------------------------
    pub fn condition(&self, p: &[u8; 8]) -> String {
        let t = ptype(p);
        let mut s = String::from("      AND ");
        let eq = cond_eq(p[0]);
        match t {
            0 => return String::new(),
            1 | 3 | 4 | 13 | 15 | 18 | 19 => {}
            2 => {
                s += &format!("TIME OF DAY IS {eq}");
                if p[1] > 0 {
                    s += &self.cust_name((p[1] & 0x7F) as i32);
                } else {
                    s += &clock12(p[2], p[3]);
                }
            }
            5 => {
                s += "THE DAY(S) OF THE WEEK IS/ARE ";
                for i in 0..7 {
                    if p[1] & (1 << i) != 0 {
                        s += ["S", "M", "T", "W", "T", "F", "S"][i];
                    } else {
                        s += "-";
                    }
                }
            }
            6 => {
                s += &format!("THE MONTH IS {eq}");
                if p[2] & 0x80 != 0 {
                    s += &self.cust_name((p[2] & 0x7F) as i32);
                } else if p[0] & 7 == 7 {
                    s += &p[1].to_string();
                } else {
                    s += MONTHS
                        .get((p[1] as usize).wrapping_sub(1))
                        .copied()
                        .unwrap_or("");
                }
            }
            7 => {
                s += &format!("THE DAY OF THE MONTH IS {eq}");
                if p[2] & 0x80 != 0 {
                    s += &self.cust_name((p[2] & 0x7F) as i32);
                } else {
                    s += &p[1].to_string();
                }
            }
            8 => {
                s += &format!("THE YEAR IS {eq}");
                if p[2] & 0x80 != 0 {
                    s += &self.cust_name((p[2] & 0x7F) as i32);
                } else {
                    s += &(2000 + p[1] as i32).to_string();
                }
            }
            9 | 10 => {
                s += &format!("THE TIME IS {eq}");
                s += &sun(p, false);
            }
            11 => {
                let ev = p[2] as i32 * 256 + p[3] as i32;
                let Some(row) = self.event_parts(ev) else {
                    return format!("{s}{}", unknown_event(ev));
                };
                let col = usize::from(p[1] == 0); // pre [5]/[6], post [7]/[8]
                let name = self.get_name(row[0].chars().skip(7).collect());
                let pre = &row[5 + col];
                let post = &row[7 + col];
                if pre == "***" {
                    s += post;
                } else {
                    if !pre.is_empty() {
                        s += pre;
                        s += " ";
                    }
                    s += &name;
                    if !post.is_empty() {
                        s += " ";
                        s += post;
                    }
                }
            }
            12 => {
                s += &self.zone_name(p[3] as i32);
                s += " IS ";
                self.zone_english(p, &mut s);
            }
            14 => s += &self.lighting_condition(p),
            16 | 23 => {
                s += &self.cntr_name(p[1] as i32);
                s += " IS ";
                s += match p[0] & 7 {
                    1 => "EQUAL TO ",
                    2 => "NOT EQUAL TO ",
                    4 => "LESS THAN ",
                    3 => "GREATER THAN ",
                    _ => "",
                };
                if t == 16 {
                    s += &(p[2] as i32 * 256 + p[3] as i32).to_string();
                } else {
                    s += &self.cust_name(p[3] as i32);
                }
            }
            17 => {
                s += &format!("LAST USER WAS {}", self.user_name(p[3] as i32));
            }
            21 => {
                let mut t2 = format!("{} ", self.tstat_name(p[1] as i32));
                match p[3] & 7 {
                    0 => {
                        t2 += "IS SET TO ";
                        t2 += match p[2] {
                            0 => "OFF",
                            1 => "HEAT",
                            2 => "COOL",
                            3 => "AUTO",
                            _ => "",
                        };
                    }
                    1 => {
                        t2 += "HOLD IS ";
                        t2 += if p[2] > 0 { "ON" } else { "OFF" };
                    }
                    2 => {
                        t2 += "FAN IS SET TO ";
                        t2 += if p[2] > 0 { "ON" } else { "AUTO" };
                    }
                    k @ 3..=5 => {
                        t2 += match k {
                            3 => "ACTUAL TEMPERATURE IS ",
                            4 => "COOLING DESIRED TEMP IS ",
                            _ => "HEATING DESIRED TEMP IS ",
                        };
                        t2 += match p[0] & 7 {
                            2 => "ANY VALUE OTHER THAN ",
                            4 => "LESS THAN ",
                            3 => "GREATER THAN ",
                            _ => "",
                        };
                        t2 += &self.temp_operand(p[3] & 0x80 != 0, p[3] & 0x40 != 0, p[2]);
                    }
                    _ => {}
                }
                s += &t2;
            }
            22 => {
                let mut t2 = if p[2] & 1 > 0 {
                    format!("{} ", self.zone_name(p[1] as i32))
                } else {
                    format!("{} ", self.keypad_name(p[1] as i32))
                };
                t2 += "ACTUAL TEMPERATURE IS ";
                t2 += match p[0] & 7 {
                    2 => "ANY VALUE OTHER THAN ",
                    4 => "LESS THAN ",
                    3 => "GREATER THAN ",
                    _ => "",
                };
                if p[2] & 0x80 > 0 {
                    t2 += &self.cust_name(p[3] as i32);
                } else if p[2] & 0x40 > 0 {
                    t2 += &self.cntr_name(p[3] as i32);
                } else {
                    // software 4.2.4 or older stores the plain value; newer files carry an offset
                    let v = if p[2] > 0 {
                        p[3] as i32 - 60
                    } else {
                        p[3] as i32 - 40
                    };
                    t2 += &format!("{} DEG. F. ({} DEG. C.)", v, f_to_c(v));
                }
                s += &t2;
            }
            _ => return String::new(),
        }
        s
    }

    fn lighting_condition(&self, p: &[u8; 8]) -> String {
        let item = p[1] as i32 + 1;
        let mut s = format!("{} ", self.light_name(item));
        let fmt = self.light_format(item);
        let ty = self.light_type(item);
        let tmp: String = if fmt != 3 && ty == 2 {
            if p[2] == 0 {
                (if p[0] & 7 == 3 { "IS ON" } else { "IS OFF" }).to_string()
            } else {
                let mut t = String::from("IS SET ");
                t += match p[0] & 7 {
                    1 => "AT LEVEL ",
                    2 => "AT ANY LEVEL OTHER THAN ",
                    4 => "AT A LEVEL LESS THAN ",
                    3 => "AT A LEVEL GREATER THAN ",
                    _ => "",
                };
                t += &format!("{}%", p[2]);
                t
            }
        } else if fmt == 3 && ty == 2 {
            if p[2] < 2 {
                (if p[2] == 1 || p[0] & 7 == 3 {
                    "IS ON"
                } else {
                    "IS OFF"
                })
                .to_string()
            } else {
                format!("IS SET TO SCENE {}", choose(&SCENES, p[2] as usize))
            }
        } else if p[2] > 0 || p[0] & 7 == 3 {
            "IS ON".to_string()
        } else {
            "IS OFF".to_string()
        };
        s += &tmp;
        s
    }

    // ---- result (THEN) --------------------------------------------------------------------------------------------
    /// `for_time`: ElkRP's `ForExpTime` (0 none, below 0 the custom value, above 0 seconds).
    pub fn result(&self, p: &[u8; 8], for_time: i32) -> String {
        let mut t = String::from("            THEN ");
        let a = self.area_name((p[4] & 7) as i32 + 1);
        match rtype(p) {
            0 | 1 | 6 | 11 => return String::new(),
            2 => {
                t += if p[4] & 7 == 0 {
                    "DISARM AREA(S) "
                } else {
                    "ARM AREA(S) "
                };
                let mut list = String::new();
                for i in 0..8 {
                    if p[5] & (1 << i) > 0 {
                        list += &format!("{}, ", i + 1);
                    }
                }
                // ElkRP always cuts the last two characters, also when no area is set
                t += &list;
                t.truncate(t.len() - 2);
                t += match p[4] & 7 {
                    1 => " TO AWAY",
                    2 => " TO STAY",
                    3 => " TO STAY INSTANT",
                    4 => " TO NIGHT",
                    5 => " TO NIGHT INSTANT",
                    6 => " TO VACATION",
                    _ => "",
                };
                let d = p[6] as i32 * 256 + p[7] as i32;
                if d == 0 {
                    t += " IMMEDIATELY";
                } else {
                    t += " AFTER ";
                    let h = d / 3600;
                    let m = d % 3600 / 60;
                    if h > 0 {
                        t += &format!("{}, ", eng(h, "HR"));
                    }
                    if m > 0 {
                        t += &eng(m, "MIN");
                    }
                    if t.ends_with(", ") {
                        t.truncate(t.len() - 2);
                    }
                }
            }
            3 => {
                if p[5] == 255 {
                    t += "RESET SMOKE POWER";
                    return t;
                }
                t += if p[4] & 6 == 4 { "TOGGLE " } else { "TURN " };
                t += &self.output_name(p[5] as i32);
                if p[4] & 6 == 2 {
                    t += " ON";
                } else if p[4] & 6 == 0 {
                    t += " OFF";
                }
                let secs = if for_time > 0 {
                    for_time
                } else {
                    p[6] as i32 * 256 + p[7] as i32
                };
                if secs > 0 {
                    t += " FOR ";
                    if for_time == 0 && p[6] & 0x80 == 128 {
                        t += &self.cust_name((p[7] & 0x7F) as i32);
                    } else {
                        t += &duration(secs);
                    }
                    if p[4] & 1 == 1 {
                        t += ", RESTART TIMER IF RUNNING";
                    }
                }
            }
            4 => {
                let item = p[5] as i32 + 1;
                let fmt = self.light_format(item);
                let ty = self.light_type(item);
                let name = self.light_name(item);
                let fade = (ty == 2 || ty == 1) && fmt == 4;
                let fr = format!(", FADE RATE = {}", p[4] & 7);
                match p[6] {
                    1 => {
                        t += &format!(
                            "TURN ALL UNITS OFF, HOUSECODE {}",
                            choose(&HOUSES, (p[5] / 16) as usize + 1)
                        )
                    }
                    2 => {
                        t += &format!(
                            "TURN ALL LIGHTS ON, HOUSECODE {}",
                            choose(&HOUSES, (p[5] / 16) as usize + 1)
                        )
                    }
                    7 => {
                        t += &format!(
                            "TURN ALL LIGHTS OFF, HOUSECODE {}",
                            choose(&HOUSES, (p[5] / 16) as usize + 1)
                        )
                    }
                    3 => {
                        t += &format!("TURN {name} ON");
                        if fade {
                            t += &fr;
                        }
                    }
                    4 => {
                        t += &format!("TURN {name} OFF");
                        if fade {
                            t += &fr;
                        }
                    }
                    5 => t += &format!("DIM {name} BY {} STEPS", p[7]),
                    6 => t += &format!("BRIGHTEN {name} BY {} STEPS", p[7]),
                    9 => {
                        t += &format!("SET {name} TO ");
                        if fmt == 3 {
                            t += &format!("SCENE {}", choose(&SCENES, p[7] as usize));
                        } else if fade {
                            t += &format!("{}% BRIGHT, FADE RATE = {}", p[7], p[4] & 7);
                        } else {
                            t += &format!("{}% BRIGHT", p[7]);
                        }
                    }
                    16 => {
                        t += &format!("TOGGLE STATE OF {name}");
                        if fade {
                            t += &fr;
                        }
                    }
                    8 => t += &format!("SET {name} TO {}% BRIGHT, FADE RATE = {}", p[7], p[4] & 7),
                    _ => {}
                }
                if for_time != 0 {
                    t += " FOR ";
                    if for_time < 0 {
                        t += &self.cust_name(for_time.abs() & 0x7F);
                    } else {
                        t += &duration(for_time);
                    }
                }
            }
            5 => {
                let n = p[6] as i32 * 256 + p[7] as i32;
                if n < 209 {
                    t += &format!("ANNOUNCE {}", self.zone_name(n));
                } else {
                    t += &format!("ANNOUNCE {}", self.vm_name(n - 208));
                }
            }
            19 => t += &format!("ANNOUNCE {}", self.task_name(p[7] as i32)),
            21 => {
                t += &format!(
                    "ANNOUNCE {}",
                    self.light_name(p[6] as i32 * 256 + p[7] as i32)
                )
            }
            20 => t += &format!("ANNOUNCE {}", self.output_name(p[7] as i32)),
            23 => {
                t += "ANNOUNCE TEMPERATURE READING OF ";
                t += &match p[5] {
                    0 => self.keypad_name(p[7] as i32),
                    1 => self.zone_name(p[7] as i32),
                    2 => self.tstat_name(p[7] as i32),
                    _ => String::new(),
                };
            }
            7 => {
                let ptr = (p[6] & 7) as usize * 256 + p[7] as usize;
                t += &format!("SEND THE FOLLOWING TEXT: \"{}\"", self.text_at(ptr));
                t += &format!(" THROUGH PORT {}", p[5]);
            }
            8 => {
                let ptr = (p[6] & 7) as usize * 256 + p[7] as usize;
                if p[5] == 0 && p[6] == 0 && p[7] == 0 {
                    t += " CLEAR PREVIOUSLY DISPLAYED MESSAGES ON KEYPADS IN ";
                    t += &a;
                } else {
                    let txt: String = self.text_at(ptr).chars().take(32).collect();
                    t += &format!("DISPLAY \"{txt}\" IN {a}");
                    let n = (p[5] as i32 & 0x1F) * 32 + (p[6] / 8) as i32;
                    if n == 0 {
                        t += " INDEFINITELY";
                    } else {
                        t += &format!(" FOR {n} SECONDS");
                    }
                    if p[5] & 0x40 != 0 {
                        t += ", [*] CLEARS";
                    }
                    if p[5] & 0x20 != 0 {
                        t += ", BEEP";
                    }
                }
            }
            9 => {
                let n = p[6] as i32 * 256 + p[7] as i32;
                match p[4] & 7 {
                    0 => t += "SET ",
                    1 => t += &format!("ADD {n} TO "),
                    2 => t += &format!("SUBTRACT {n} FROM "),
                    3 => t += "TOGGLE ",
                    _ => {}
                }
                t += &self.cntr_name(p[5] as i32);
                if p[4] & 7 == 0 {
                    t += &format!(" TO {n}");
                }
                if p[4] & 7 == 3 {
                    t += " BETWEEN 0 AND 1";
                }
            }
            25 => {
                t += "SET ";
                t += &self.cntr_name(p[7] as i32);
                t += " TO TEMPERATURE OF ";
                match p[5] & 3 {
                    0 => t += &self.keypad_name(p[6] as i32),
                    1 => t += &self.zone_name(p[6] as i32),
                    _ => t += &format!("SET {}", self.tstat_name(p[6] as i32)),
                }
            }
            10 => t += &format!("ACTIVATE {}", self.task_name(p[5] as i32)),
            12 => {
                t += if p[5] == 0 {
                    "STOP KEYPAD BEEPS IN "
                } else {
                    "START KEYPAD BEEPS IN "
                };
                t += &a;
            }
            13 => {
                let n = p[4] & 7;
                t += &format!("CHIRP THE OUTSIDE SIREN {n} TIME");
                if n > 1 {
                    t += "S";
                }
            }
            15 | 16 => {
                t += &format!(
                    "EXPECT {a} TO {} WITHIN ",
                    if rtype(p) == 15 { "ARM" } else { "DISARM" }
                );
                let n = bank_round((p[6] as i32 * 256 + p[7] as i32) as f64 / 60.0);
                let h = n / 60;
                let m = n % 60;
                if h > 0 {
                    t += &format!("{h} HOUR{}", if h == 1 { " " } else { "S " });
                    if m > 0 {
                        t += "AND ";
                    }
                }
                if m > 0 {
                    t += &format!("{m} MINUTE");
                    if m > 1 {
                        t += "S";
                    }
                }
            }
            17 => {
                t += match p[5] {
                    0 => "DISABLE CHIME IN ",
                    1 => "SET CHIME TO DING ONLY IN ",
                    2 => "SET CHIME TO VOICE ONLY IN ",
                    3 => "SET CHIME TO DING AND VOICE IN ",
                    _ => "",
                };
                t += &a;
            }
            18 => {
                t += "DIAL ";
                t += &self.tele_name((p[5] & 0xF) as i32 + 1);
                t += " AND ANNOUNCE ";
                let n = p[6] as i32 * 256 + p[7] as i32;
                if n > 0 {
                    t += &self.vm_name(n - 208);
                } else {
                    t += "THE MESSAGE PROGRAMMED FOR THIS NUMBER";
                }
            }
            22 => {
                t += &format!("SET {} ", self.tstat_name(p[6] as i32));
                match p[7] / 2 {
                    0 => {
                        t += match p[5] {
                            0 => "OFF",
                            1 => "TO HEAT",
                            2 => "TO COOL",
                            3 => "TO AUTO",
                            _ => "",
                        }
                    }
                    1 => {
                        t += "HOLD TO ";
                        t += if p[5] > 0 { "ON" } else { "OFF" };
                    }
                    2 => {
                        t += "FAN TO ";
                        t += if p[5] > 0 { "ON" } else { "AUTO" };
                    }
                    k @ (4 | 5) => {
                        t += if k == 4 {
                            "COOLING DESIRED TEMP TO "
                        } else {
                            "HEATING DESIRED TEMP TO "
                        };
                        if p[7] & 1 != 0 {
                            t += &self.cust_name(p[5] as i32);
                        } else {
                            t += &format!("{} DEG. F. ({} DEG. C.)", p[5], f_to_c(p[5] as i32));
                        }
                    }
                    _ => {}
                }
            }
            24 => {
                t += if p[4] & 1 == 1 { "DISABLE " } else { "ENABLE " };
                t += &self.user_name(p[7] as i32);
            }
            26 => {
                t += if p[5] == 0 {
                    "ENABLE NON-ALARM VOICE MESSAGES"
                } else {
                    "DISABLE NON-ALARM VOICE MESSAGES"
                }
            }
            27 => t += &format!("SEND EMAIL MESSAGE {} TO Email {}", p[7], p[7]),
            28 => {
                t += if p[5] == 1 { "BYPASS " } else { "UNBYPASS " };
                let n = p[6] as i32 * 256 + p[7] as i32;
                match n {
                    0 => t += &format!("ALL BURGLAR ZONES IN {a}"),
                    999 => t += &format!("ALL NON-SECURE BURGLAR ZONES IN {a}"),
                    _ => t += &self.zone_name(n),
                }
            }
            29 => {
                let n = (p[4] & 7) as i32 * 8 + (p[5] / 32) as i32;
                t += match n {
                    0 => "TURN OFF ",
                    1 => "TURN ON ",
                    2 => "TOGGLE POWER TO ",
                    3 => "SELECT NEXT SOURCE FOR ",
                    4 => "SET SOURCE FOR ",
                    5 => "SEND 'PREVIOUS' COMMAND TO ",
                    6 => "SEND 'NEXT' COMMAND TO ",
                    7 => "TURN DOWN VOLUME OF ",
                    8 => "TURN UP VOLUME OF ",
                    9 => "MUTE ",
                    10 => "SET VOLUME OF ",
                    11 => "SEND 'PLAY' COMMAND TO ",
                    12 => "SEND 'PAUSE' COMMAND TO ",
                    13 => "SEND 'STOP' COMMAND TO ",
                    14 => "SELECT FAVORITE 1 IN ",
                    15 => "SELECT FAVORITE 2 IN ",
                    16 => "SEND 'MINUS' [ - ] COMMAND TO ",
                    17 => "SEND 'PLUS' [+] COMMAND TO ",
                    18 => "TURN ALL AUDIO ZONES OFF",
                    19 => "TURN ALL AUDIO ZONES ON",
                    _ => "[UNKNOWN AUDIO COMMAND]",
                };
                if (0..18).contains(&n) {
                    // audio zone names live in the M1XEP configuration, not in the export file
                    t += &format!("Audio Zone {}", p[6]);
                }
                if n == 4 {
                    t += &format!(" TO Audio Source {}", p[5] & 0x1F);
                } else if n == 10 {
                    t += &format!(" TO {}%", p[7]);
                }
            }
            30 => t += &format!("SHOW AREA ARMING MENU ON {}", self.keypad_name(p[5] as i32)),
            _ => return String::new(),
        }
        t
    }
}

const SCENES: [&str; 12] = ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L"];
const HOUSES: [&str; 16] = [
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P",
];

/// VB `Interaction.Choose` (1-based; out of range gives nothing)
fn choose(list: &[&str], i: usize) -> String {
    if i >= 1 && i <= list.len() {
        list[i - 1].to_string()
    } else {
        String::new()
    }
}

fn cond_eq(b: u8) -> &'static str {
    match b & 7 {
        2 => "NOT ",
        3 => "LATER THAN ",
        4 => "EARLIER THAN ",
        7 => "A MULTIPLE OF ",
        _ => "",
    }
}

fn clock12(h: u8, m: u8) -> String {
    let hh = if h == 0 {
        12
    } else if h > 12 {
        h - 12
    } else {
        h
    };
    format!("{hh}:{m:02}{}", if h >= 12 { " PM" } else { " AM" })
}

/// Sunrise / sunset (trigger: after "WHENEVER ", "THE TIME IS "; condition: the caller wrote it)
fn sun(p: &[u8; 8], trigger: bool) -> String {
    let mut s = String::new();
    let mut n = p[2] as i32 * 256 + p[3] as i32;
    if n != 0 {
        if trigger {
            s += "THE TIME IS ";
        }
        if p[2] > 127 {
            n = 65536 - n;
        }
        if n > 59 {
            s += &format!("{} HOUR{}", n / 60, if n / 60 > 1 { "S " } else { " " });
        }
        if n % 60 != 0 {
            if n > 59 {
                s += "AND ";
            }
            s += &format!("{} MINUTE{}", n % 60, if n % 60 > 1 { "S " } else { " " });
        }
        s += if p[2] & 0x80 != 0 {
            "BEFORE "
        } else {
            "AFTER "
        };
    } else if !trigger {
        s += "AT ";
    }
    s += if ptype(p) == 9 { "SUNRISE" } else { "SUNSET" };
    s
}

fn eng(n: i32, unit: &str) -> String {
    format!("{n} {unit}{}", if n > 1 { "S" } else { "" })
}

fn duration(secs: i32) -> String {
    let d = secs / 86400;
    let h = secs % 86400 / 3600;
    let m = secs % 86400 % 3600 / 60;
    let s = secs % 86400 % 3600 % 60;
    let mut t = String::new();
    if d > 0 {
        t += &format!("{}, ", eng(d, "DAY"));
    }
    if h > 0 {
        t += &format!("{}, ", eng(h, "HR"));
    }
    if m > 0 {
        t += &format!("{}, ", eng(m, "MIN"));
    }
    if s > 0 {
        t += &eng(s, "SEC");
    }
    if t.ends_with(", ") {
        t.truncate(t.len() - 2);
    }
    t
}

/// `PrintableText`: the stored string has `{ff:xx:xx:xx}` for inserted values.
pub fn printable(raw: &str) -> String {
    let mut out = String::new();
    let mut rest = raw;
    while let Some(at) = rest.find("{ff:") {
        out += &rest[..at];
        let tail = &rest[at..];
        let ok = tail.len() >= 14 && tail.as_bytes()[13] == b'}';
        if !ok {
            out += "{ff:";
            rest = &tail[4..];
            continue;
        }
        let g = |i: usize| u8::from_str_radix(&tail[4 + 3 * i..6 + 3 * i], 16).unwrap_or(0);
        let (k, n) = (g(0), g(2));
        let t2 = match k {
            0 => {
                if n <= 12 {
                    format!("COT{n}")
                } else {
                    format!("CS{}", n - 12)
                }
            }
            1 => format!("CTR{n}"),
            5 => format!("TEMP ZN{n}"),
            6 => format!("TEMP KP{n}"),
            _ => format!("{n}"),
        };
        let t2 = if k > 6 || k == 2 || k == 3 || k == 4 {
            String::new()
        } else {
            t2
        };
        out += &format!("\u{7f}% {t2} %\u{7f}");
        rest = &tail[14..];
    }
    out += rest;
    out
}

/// Wording for an event number the table does not have (or, without the table, for every event).
fn unknown_event(ev: i32) -> String {
    if crate::rules::event_table_present() {
        "Unknown event".to_string()
    } else {
        format!("Event {ev}")
    }
}

/// One row of ElkRP's event table: `[description, TrigPrePro, TrigPreCon, TrigPostPro, TrigPostCon, CondPrePro, CondPreCon,
/// CondPostPro, CondPostCon]`.
fn event_row(ev: i32) -> Option<[String; 9]> {
    static ROWS: std::sync::OnceLock<HashMap<i32, [String; 9]>> = std::sync::OnceLock::new();
    let m = ROWS.get_or_init(|| {
        let mut m = HashMap::new();
        for line in crate::automation::AutomationMeta::load_lines(crate::automation::EVENT_TSV) {
            let t: Vec<&str> = line.trim_end_matches('\r').split('\t').collect();
            if t.len() < 2 {
                continue;
            }
            let Some(eq) = t[1].find(" = ") else { continue };
            let Some(n) = crate::error::parse_int_loose(&t[1][..eq]) else {
                continue;
            };
            let col = |i: usize| t.get(i + 1).map_or(String::new(), |s| s.to_string());
            m.entry(n).or_insert_with(|| {
                [
                    t[1].to_string(),
                    col(1),
                    col(2),
                    col(3),
                    col(4),
                    col(5),
                    col(6),
                    col(7),
                    col(8),
                ]
            });
        }
        m
    });
    m.get(&ev).cloned()
}

/// The English of every rule of the block: one entry per rule, one string per line (WHENEVER, AND, THEN ...).
pub fn rule_lines(a: &AutomationBlock, names: &Names) -> Vec<Vec<String>> {
    let cx = Ctx { a, n: names };
    group(a)
        .into_iter()
        .map(|(start, count)| lines_of(&cx, &a.packets[start..start + count]))
        .collect()
}

/// The lines of one rule (its packets); see `WhenThen.PopulateRules`.
pub fn lines_of(cx: &Ctx, pk: &[[u8; 8]]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if pk.is_empty() {
        return out;
    }
    if matches!(ptype(&pk[0]), 18 | 19) {
        return vec!["(continuation packet)".to_string()];
    }
    out.push(cx.trigger(&pk[0]));
    let mut p = 0usize;
    while p + 1 < pk.len() && rtype(&pk[p]) == 1 && !matches!(ptype(&pk[p + 1]), 18 | 19) {
        p += 1;
        out.push(cx.condition(&pk[p]));
    }
    let show = |p: &mut usize, out: &mut Vec<String>| {
        if *p + 1 < pk.len() && ptype(&pk[*p + 1]) == 19 {
            let f = &pk[*p + 1];
            let ft = if f[1] > 0 {
                -(f[1] as i32)
            } else {
                (((f[4] as i32) * 256 + f[5] as i32)
                    .wrapping_mul(256)
                    .wrapping_add(f[6] as i32))
                .wrapping_mul(256)
                .wrapping_add(f[7] as i32)
            };
            out.push(cx.result(&pk[*p], ft));
            *p += 2;
        } else {
            out.push(cx.result(&pk[*p], 0));
            *p += 1;
        }
    };
    if rtype(&pk[p]) > 1 {
        show(&mut p, &mut out);
    } else {
        p += 1;
    }
    while p < pk.len() && ptype(&pk[p]) == 18 {
        show(&mut p, &mut out);
    }
    out.into_iter()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// `n% of Rules/Text space used` (ElkRP rounds like VB: ties to even).
pub fn percent_used(rule_packets: usize, text_packets: usize) -> i32 {
    bank_round(((rule_packets + text_packets) * 100) as f64 / 528.0)
}

/// English of all rules of a parsed account file (None when the file has no automation block).
pub fn file_rule_lines(f: &AccountFile) -> Option<Vec<Vec<String>>> {
    f.automation
        .as_ref()
        .map(|a| rule_lines(a, &Names::from_file(f)))
}

/// The answer of `/api/rules/english`: the wording of every rule and the packet budget of the file.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RulesEnglish {
    pub rules: Vec<Vec<String>>,
    pub rule_packets_used: i32,
    pub text_packets_used: i32,
    pub percent: i32,
}

pub fn english_report(f: &AccountFile) -> Option<RulesEnglish> {
    let a = f.automation.as_ref()?;
    Some(RulesEnglish {
        rules: rule_lines(a, &Names::from_file(f)),
        rule_packets_used: a.rule_packets_used as i32,
        text_packets_used: a.text_packets_used as i32,
        percent: percent_used(a.rule_packets_used, a.text_packets_used),
    })
}

/// `/api/meta` member `eventWords` (Rust editor only): event number to the eight phrases of ElkRP's event table
/// (TrigPrePro, TrigPreCon, TrigPostPro, TrigPostCon, CondPrePro, CondPreCon, CondPostPro, CondPostCon), in file order.
pub fn event_words_json() -> String {
    let mut out = String::from("{");
    let mut first = true;
    for (n, _) in event_names().iter() {
        if let Some(row) = event_row(n) {
            if !first {
                out.push(',');
            }
            first = false;
            out += &format!("\"{n}\":{}", crate::json::to_string(&row[1..].to_vec()));
        }
    }
    out.push('}');
    out
}
