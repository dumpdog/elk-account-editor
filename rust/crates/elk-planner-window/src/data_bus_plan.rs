//! Port of `Elk.Protocol/DataBusPlan.cs`: a data bus plan for an Elk M1 system and the rules it is checked against.
//!
//! `check_json` is `POST /api/databus/check`: a `BusPlan` as JSON in, a `PlanResult` as JSON out, byte-identical to
//! what System.Text.Json (web defaults, camelCase) produces in the C# server.
//!
//! Sources (as in the C#): ELK's page "5 Tips for the M1 Data Bus", the ELK-M1DBH and ELK-M1DBHR instruction sheets and the
//! zone expander address tables.

use crate::jsonw;
use serde_json::{Map, Value};

pub const MAX_BUS_FT: f64 = 4000.0;
pub const PTC_MA: f64 = 1250.0;
pub const HUB_FEED_RIBBON_IN: f64 = 24.0;

// ---------------------------------------------------------------------------------------------------------------------------------
// Plan types
// ---------------------------------------------------------------------------------------------------------------------------------

/// A device on the bus. `ma` is the current it draws from the bus supply; 0 means "not entered".
#[derive(Debug, Clone, PartialEq)]
pub struct BusDevice {
    pub model: String,
    pub address: i32,
    pub name: String,
    pub ma: f64,
}

impl Default for BusDevice {
    fn default() -> Self {
        BusDevice {
            model: "Keypad".into(),
            address: 1,
            name: String::new(),
            ma: 0.0,
        }
    }
}

/// A cable run with devices in daisy chain order, nearest the hub or control first. The last device is the end of the run.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BusRun {
    pub name: String,
    pub length_ft: f64,
    pub devices: Vec<BusDevice>,
}

/// An ELK-M1DBH. Home runs go in jacks J1, J2 ... in order; `crossover_ft` is the cable to the next hub.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BusHub {
    pub home_runs: Vec<BusRun>,
    pub crossover_ft: f64,
    /// Last hub only: end the bus with the terminating jumper on the device on the last jack instead of the 120 ohm plug.
    pub terminate_at_device: bool,
}

impl BusHub {
    pub fn ends_at_device(&self, last: bool) -> bool {
        last && !self.home_runs.is_empty()
            && (self.terminate_at_device || self.home_runs.len() >= 9)
    }
}

/// One of the four managed branches of an ELK-M1DBHR: one or two home runs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BusBranch {
    pub runs: Vec<BusRun>,
}

/// One of the (at most two) runs that connect to the control's data bus. Kind: "direct", "dbh" or "dbhr".
#[derive(Debug, Clone, PartialEq)]
pub struct MainRun {
    pub kind: String,
    pub name: String,
    pub run: BusRun,
    pub feed_ft: f64,
    pub hubs: Vec<BusHub>,
    pub branches: Vec<BusBranch>,
}

impl Default for MainRun {
    fn default() -> Self {
        MainRun {
            kind: "direct".into(),
            name: String::new(),
            run: BusRun::default(),
            feed_ft: 1.0,
            hubs: Vec::new(),
            branches: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BusPlan {
    pub runs: Vec<MainRun>,
    /// Zone groups (2 to 13) that the account has as wireless.
    pub wireless_groups: Vec<i32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BusModel {
    pub id: &'static str,
    pub label: &'static str,
    pub type_: i32,
    pub min_address: i32,
    pub max_address: i32,
    pub note: &'static str,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlanIssue {
    pub level: String,
    pub text: String,
    pub where_: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChecklistGroup {
    pub title: String,
    pub items: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlanResult {
    pub issues: Vec<PlanIssue>,
    pub checklist: Vec<ChecklistGroup>,
    pub main_bus_ft: f64,
    pub limit_ft: f64,
    pub current_ma: f64,
    pub jp3: bool,
    pub devices: i32,
}

impl Default for PlanResult {
    fn default() -> Self {
        PlanResult {
            issues: Vec::new(),
            checklist: Vec::new(),
            main_bus_ft: 0.0,
            limit_ft: MAX_BUS_FT,
            current_ma: 0.0,
            jp3: false,
            devices: 0,
        }
    }
}

impl PlanResult {
    pub fn errors(&self) -> i32 {
        self.issues.iter().filter(|i| i.level == "error").count() as i32
    }
    pub fn warnings(&self) -> i32 {
        self.issues.iter().filter(|i| i.level == "warn").count() as i32
    }

    /// The JSON the C# server returns: camelCase, declaration order, System.Text.Json's default escaping.
    pub fn to_json(&self) -> String {
        let mut o = String::new();
        o.push_str("{\"issues\":[");
        for (i, x) in self.issues.iter().enumerate() {
            if i > 0 {
                o.push(',');
            }
            o.push_str("{\"level\":");
            jsonw::push_str(&mut o, &x.level);
            o.push_str(",\"text\":");
            jsonw::push_str(&mut o, &x.text);
            o.push_str(",\"where\":");
            jsonw::push_str(&mut o, &x.where_);
            o.push('}');
        }
        o.push_str("],\"checklist\":[");
        for (i, g) in self.checklist.iter().enumerate() {
            if i > 0 {
                o.push(',');
            }
            o.push_str("{\"title\":");
            jsonw::push_str(&mut o, &g.title);
            o.push_str(",\"items\":[");
            for (j, it) in g.items.iter().enumerate() {
                if j > 0 {
                    o.push(',');
                }
                jsonw::push_str(&mut o, it);
            }
            o.push_str("]}");
        }
        o.push_str("],\"mainBusFt\":");
        jsonw::push_f64(&mut o, self.main_bus_ft);
        o.push_str(",\"limitFt\":");
        jsonw::push_f64(&mut o, self.limit_ft);
        o.push_str(",\"currentMa\":");
        jsonw::push_f64(&mut o, self.current_ma);
        o.push_str(",\"jp3\":");
        o.push_str(if self.jp3 { "true" } else { "false" });
        o.push_str(&format!(
            ",\"devices\":{},\"errors\":{},\"warnings\":{}}}",
            self.devices,
            self.errors(),
            self.warnings()
        ));
        o
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// Tables
// ---------------------------------------------------------------------------------------------------------------------------------

/// The device types ("Type 1" ...) are ELK's grouping: an address may be used once per type.
pub const MODELS: [BusModel; 11] = [
    BusModel {
        id: "Keypad",
        label: "Keypad (M1KP2, M1KP3, M1KPNAV, ...)",
        type_: 1,
        min_address: 1,
        max_address: 16,
        note: "Type 1",
    },
    BusModel {
        id: "M1KAM",
        label: "M1KAM access module",
        type_: 1,
        min_address: 1,
        max_address: 16,
        note: "Type 1: shares the address list with the keypads",
    },
    BusModel {
        id: "PowerSupply",
        label: "Supervised power supply",
        type_: 1,
        min_address: 1,
        max_address: 16,
        note: "Type 1: shares the address list with the keypads",
    },
    BusModel {
        id: "M1XIN",
        label: "M1XIN hardwired input expander (16 zones)",
        type_: 2,
        min_address: 2,
        max_address: 13,
        note: "Type 2: the address sets the 16 zones it uses (address 2 = zones 17-32)",
    },
    BusModel {
        id: "M1XRFTWM",
        label: "M1XRFTWM wireless transceiver",
        type_: 2,
        min_address: 2,
        max_address: 5,
        note: "Type 2: primary at address 2, up to three more at 3, 4 and 5",
    },
    BusModel {
        id: "M1XRFEG",
        label: "M1XRFEG wireless input expander",
        type_: 2,
        min_address: 2,
        max_address: 13,
        note: "Type 2 (address range not confirmed)",
    },
    BusModel {
        id: "M1XRF2H",
        label: "M1XRF2H wireless input expander",
        type_: 2,
        min_address: 2,
        max_address: 13,
        note: "Type 2 (address range not confirmed)",
    },
    BusModel {
        id: "M1XOVR",
        label: "M1XOVR output expander",
        type_: 3,
        min_address: 1,
        max_address: 13,
        note: "Type 3",
    },
    BusModel {
        id: "M1XSP",
        label: "M1XSP serial port expander",
        type_: 5,
        min_address: 1,
        max_address: 7,
        note: "Type 5",
    },
    BusModel {
        id: "M1XSLU",
        label: "M1XSLU serial lighting expander",
        type_: 5,
        min_address: 1,
        max_address: 7,
        note: "Type 5",
    },
    BusModel {
        id: "M1PCSPIM",
        label: "M1PCSPIM power line interface",
        type_: 5,
        min_address: 1,
        max_address: 7,
        note: "Type 5",
    },
];

/// S1 to S4 for a hardwired expander (M1XIN) address, in the C# dictionary's insertion order.
pub const ZONE_EXPANDER_SWITCHES: [(i32, &str); 12] = [
    (2, "S1 off, S2 on, S3 off, S4 off"),
    (3, "S1 on, S2 on, S3 off, S4 off"),
    (4, "S1 off, S2 off, S3 on, S4 off"),
    (5, "S1 on, S2 off, S3 on, S4 off"),
    (6, "S1 off, S2 on, S3 on, S4 off"),
    (7, "S1 on, S2 on, S3 on, S4 off"),
    (8, "S1 off, S2 off, S3 off, S4 on"),
    (9, "S1 on, S2 off, S3 off, S4 on"),
    (10, "S1 off, S2 on, S3 off, S4 on"),
    (11, "S1 on, S2 on, S3 off, S4 on"),
    (12, "S1 off, S2 off, S3 on, S4 on"),
    (13, "S1 on, S2 off, S3 on, S4 on"),
];

pub const TRANSCEIVER_SWITCHES: [(i32, &str); 4] = [
    (2, "S1 off, S2 on, S3 off, S4 off"),
    (3, "S1 on, S2 off, S3 off, S4 off"),
    (4, "S1 off, S2 off, S3 on, S4 off"),
    (5, "S1 on, S2 off, S3 on, S4 off"),
];

fn lookup(table: &[(i32, &'static str)], address: i32) -> Option<&'static str> {
    table.iter().find(|(a, _)| *a == address).map(|(_, s)| *s)
}

/// Case-insensitive model lookup (`DataBusPlanner.Model`).
pub fn model(id: &str) -> Option<&'static BusModel> {
    MODELS.iter().find(|m| m.id.eq_ignore_ascii_case(id))
}

/// Zones a hardwired expander at this address uses.
pub fn zones_at(address: i32) -> (i32, i32) {
    (16 * (address - 1) + 1, 16 * address)
}

/// `GET /api/meta` `busModels`, as System.Text.Json writes it.
pub fn models_json() -> String {
    let mut o = String::from("[");
    for (i, m) in MODELS.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        o.push_str("{\"id\":");
        jsonw::push_str(&mut o, m.id);
        o.push_str(",\"label\":");
        jsonw::push_str(&mut o, m.label);
        o.push_str(&format!(
            ",\"type\":{},\"minAddress\":{},\"maxAddress\":{},\"note\":",
            m.type_, m.min_address, m.max_address
        ));
        jsonw::push_str(&mut o, m.note);
        o.push('}');
    }
    o.push(']');
    o
}

/// `GET /api/meta` `zoneExpanderSwitches` (a `Dictionary<int,string>`: keys become strings, insertion order).
pub fn zone_expander_switches_json() -> String {
    let mut o = String::from("{");
    for (i, (a, s)) in ZONE_EXPANDER_SWITCHES.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        o.push_str(&format!("\"{}\":", a));
        jsonw::push_str(&mut o, s);
    }
    o.push('}');
    o
}

// ---------------------------------------------------------------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------------------------------------------------------------

/// `v.ToString("0.#", InvariantCulture)`. .NET formats a custom pattern from the first 15 significant digits, then rounds
/// half away from zero at the first decimal.
fn f(v: f64) -> String {
    let neg = v.is_sign_negative();
    let a = v.abs();
    let (int_digits, frac_digits): (Vec<u8>, Vec<u8>) = if a == 0.0 {
        (vec![0], vec![])
    } else {
        let sci = format!("{:.14e}", a); // d.dddddddddddddde<exp>
        let (mant, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
        let exp: i32 = exp.parse().unwrap_or(0);
        let digits: Vec<u8> = mant
            .bytes()
            .filter(|b| b.is_ascii_digit())
            .map(|b| b - b'0')
            .collect();
        let point = exp + 1; // digits before the decimal point
        if point <= 0 {
            let mut fr = vec![0u8; (-point) as usize];
            fr.extend(&digits);
            (vec![0], fr)
        } else if point as usize >= digits.len() {
            let mut ip = digits.clone();
            ip.resize(point as usize, 0);
            (ip, vec![])
        } else {
            (
                digits[..point as usize].to_vec(),
                digits[point as usize..].to_vec(),
            )
        }
    };
    let d1 = frac_digits.first().copied().unwrap_or(0);
    let d2 = frac_digits.get(1).copied().unwrap_or(0);
    // integer digits + first decimal as one number, rounded half up on the second decimal
    let mut num: Vec<u8> = int_digits.clone();
    num.push(d1);
    if d2 >= 5 {
        let mut i = num.len();
        loop {
            if i == 0 {
                num.insert(0, 1);
                break;
            }
            i -= 1;
            if num[i] == 9 {
                num[i] = 0;
            } else {
                num[i] += 1;
                break;
            }
        }
    }
    let new_d1 = num.pop().unwrap_or(0);
    let mut int_s: String = num.iter().map(|d| (b'0' + d) as char).collect();
    if int_s.is_empty() {
        int_s.push('0');
    }
    let mut s = String::new();
    if neg {
        s.push('-');
    }
    s.push_str(&int_s);
    if new_d1 != 0 {
        s.push('.');
        s.push((b'0' + new_d1) as char);
    }
    s
}

fn dev(d: &BusDevice) -> String {
    format!(
        "{}{} (address {})",
        d.model,
        if d.name.trim().is_empty() {
            String::new()
        } else {
            format!(" \"{}\"", d.name.trim())
        },
        d.address
    )
}

struct Issues(Vec<PlanIssue>);

impl Issues {
    fn add(&mut self, level: &str, text: String, w: &str) {
        self.0.push(PlanIssue {
            level: level.into(),
            text,
            where_: w.into(),
        });
    }
    fn err(&mut self, t: String, w: &str) {
        self.add("error", t, w);
    }
    fn warn(&mut self, t: String, w: &str) {
        self.add("warn", t, w);
    }
    fn info(&mut self, t: String, w: &str) {
        self.add("info", t, w);
    }
}

fn run_label(i: usize, m: &MainRun) -> String {
    format!(
        "Run {}{}",
        i + 1,
        if m.name.trim().is_empty() {
            String::new()
        } else {
            format!(" \"{}\"", m.name.trim())
        }
    )
}

// ---------------------------------------------------------------------------------------------------------------------------------
// The check
// ---------------------------------------------------------------------------------------------------------------------------------

type Placed<'a> = Vec<(&'a BusDevice, String)>;

/// `DataBusPlanner.Check`.
pub fn check(plan: &BusPlan) -> PlanResult {
    let mut r = PlanResult::default();
    let mut iss = Issues(Vec::new());
    let runs = &plan.runs;
    let mut all: Placed = Vec::new();
    if runs.is_empty() {
        iss.info(
            "Add a run to start: a plain daisy chain, a data bus hub (M1DBH) or a retrofit hub (M1DBHR).".into(),
            "",
        );
        r.issues = iss.0;
        return r;
    }
    if runs.len() > 2 {
        iss.err(
            format!("{} runs connect to the control's data bus. No more than 2 runs may connect directly (the screw terminals and the J1 and J2 connectors all count).", runs.len()),
            "plan",
        );
    }

    // ---- per run structure, lengths, terminations
    let mut main_ft = 0.0f64;
    let mut dbhr_count = 0;
    for (i, m) in runs.iter().enumerate() {
        let w = format!("run{}", i + 1);
        let label = run_label(i, m);
        match m.kind.to_lowercase().as_str() {
            "direct" => {
                if m.run.devices.is_empty() {
                    iss.err(format!("{} has no devices. A run needs at least one, and its last device is the one that is terminated.", label), &w);
                }
                if m.run.length_ft <= 0.0 {
                    iss.warn(
                        format!(
                            "{}: enter the cable length so the {} ft limit can be checked.",
                            label,
                            f(MAX_BUS_FT)
                        ),
                        &w,
                    );
                }
                main_ft += m.run.length_ft.max(0.0);
                for d in &m.run.devices {
                    all.push((d, w.clone()));
                }
                if m.run.devices.len() > 1 {
                    iss.info(format!("{}: wire the {} devices as a daisy chain, one after another. Star and T connections are not allowed.", label, m.run.devices.len()), &w);
                }
            }
            "dbh" => {
                if m.hubs.is_empty() {
                    iss.err(format!("{} has no M1DBH hub.", label), &w);
                    continue;
                }
                if m.feed_ft <= 0.0 {
                    iss.warn(
                        format!(
                            "{}: enter the length of the cable from the control to the first hub.",
                            label
                        ),
                        &w,
                    );
                }
                main_ft += m.feed_ft.max(0.0);
                if m.feed_ft > HUB_FEED_RIBBON_IN / 12.0 * 2.0 {
                    iss.warn(format!("{}: the manual recommends the M1DBH close to the control (the ELK-W018B ribbon cable is for under {} inches). The feed cable here is {} ft: use a dedicated 4-wire cable, 22 AWG or heavier.", label, f(HUB_FEED_RIBBON_IN), f(m.feed_ft)), &w);
                }
                for (h, hub) in m.hubs.iter().enumerate() {
                    let last = h == m.hubs.len() - 1;
                    let hw = format!("{}/hub{}", w, h + 1);
                    let hl = format!("{}, hub {}", label, h + 1);
                    if hub.home_runs.is_empty() {
                        iss.warn(format!("{} has no home runs.", hl), &hw);
                    }
                    if hub.home_runs.len() > 9 {
                        iss.err(
                            format!(
                                "{} has {} home runs. The hub has nine jacks (add a hub for more).",
                                hl,
                                hub.home_runs.len()
                            ),
                            &hw,
                        );
                    } else if !last && hub.home_runs.len() > 8 {
                        iss.err(format!("{} has {} home runs. The jack after the last cable must be free for the crossover cable to the next hub, so use at most 8 (move one home run to the next hub).", hl, hub.home_runs.len()), &hw);
                    }
                    if hub.ends_at_device(last) {
                        let last_run = &hub.home_runs[hub.home_runs.len() - 1];
                        if last_run.devices.is_empty() {
                            iss.err(format!("{}: the bus ends at the device on the last jack, but home run \"{}\" has no device to carry the terminating jumper.", hl, last_run.name), &hw);
                        } else {
                            iss.info(format!("{}: the bus ends at the device on the last jack, {}. Fit its terminating jumper instead of the plug, and terminate no other device.", hl, dev(&last_run.devices[last_run.devices.len() - 1])), &hw);
                        }
                    }
                    for hr in &hub.home_runs {
                        if hr.devices.is_empty() {
                            iss.err(
                                format!("{}: home run \"{}\" has no device.", hl, hr.name),
                                &hw,
                            );
                        }
                        if hr.length_ft <= 0.0 {
                            iss.warn(
                                format!("{}: enter the length of home run \"{}\".", hl, hr.name),
                                &hw,
                            );
                        }
                        if hr.devices.len() > 1 {
                            iss.info(format!("{}: home run \"{}\" has {} devices. A home run goes to one device location; devices at the same location share its terminals. Devices at different places need their own home runs.", hl, hr.name, hr.devices.len()), &hw);
                        }
                        main_ft += 2.0 * hr.length_ft.max(0.0);
                        for d in &hr.devices {
                            all.push((d, hw.clone()));
                        }
                    }
                    if !last {
                        if hub.crossover_ft <= 0.0 {
                            iss.warn(
                                format!(
                                    "{}: enter the length of the crossover cable to hub {}.",
                                    hl,
                                    h + 2
                                ),
                                &hw,
                            );
                        }
                        main_ft += 2.0 * hub.crossover_ft.max(0.0);
                    }
                }
                if m.hubs.len() > 1 {
                    iss.info(format!("{}: hubs are linked in one straight daisy chain with a crossover cable. Never connect hubs as spokes off one hub.", label), &w);
                }
            }
            "dbhr" => {
                dbhr_count += 1;
                if m.feed_ft <= 0.0 {
                    iss.warn(format!("{}: enter the length of the cable from the control to the retrofit hub.", label), &w);
                }
                if m.feed_ft > 10.0 {
                    iss.err(format!("{}: the M1DBHR must be mounted inside the M1 enclosure and cannot be remoted. {} ft of feed cable is too far.", label, f(m.feed_ft)), &w);
                } else if m.feed_ft > 3.0 {
                    iss.warn(format!("{}: the M1DBHR is connected to the control with a short 4-conductor cable and mounted in the M1 enclosure ({} ft is long).", label, f(m.feed_ft)), &w);
                }
                main_ft += m.feed_ft.max(0.0);
                if m.branches.len() != 4 {
                    iss.err(
                        format!(
                            "{}: the M1DBHR has exactly 4 branches (found {}).",
                            label,
                            m.branches.len()
                        ),
                        &w,
                    );
                }
                for (b, br) in m.branches.iter().enumerate() {
                    let bw = format!("{}/branch{}", w, b + 1);
                    let bl = format!("{}, branch {}", label, b + 1);
                    if br.runs.len() > 2 {
                        iss.err(
                            format!(
                                "{} has {} home runs. A branch can have at most 2.",
                                bl,
                                br.runs.len()
                            ),
                            &bw,
                        );
                    }
                    let mut bft = 0.0f64;
                    for hr in &br.runs {
                        if hr.devices.is_empty() {
                            iss.err(
                                format!(
                                    "{}: home run \"{}\" has no device to terminate.",
                                    bl, hr.name
                                ),
                                &bw,
                            );
                        }
                        if hr.length_ft <= 0.0 {
                            iss.warn(
                                format!("{}: enter the length of home run \"{}\".", bl, hr.name),
                                &bw,
                            );
                        }
                        bft += hr.length_ft.max(0.0);
                        for d in &hr.devices {
                            all.push((d, bw.clone()));
                        }
                    }
                    if bft > MAX_BUS_FT {
                        iss.err(
                            format!(
                                "{} is {} ft of cable. The longest a branch may be is {} ft.",
                                bl,
                                f(bft),
                                f(MAX_BUS_FT)
                            ),
                            &bw,
                        );
                    } else if bft > 0.8 * MAX_BUS_FT {
                        iss.warn(format!("{} is {} ft, close to the {} ft limit. Use 18 to 22 AWG (24 AWG is the minimum).", bl, f(bft), f(MAX_BUS_FT)), &bw);
                    }
                }
            }
            _ => iss.err(format!("{}: unknown run type \"{}\".", label, m.kind), &w),
        }
    }
    if dbhr_count > 2 {
        iss.err("More than two M1DBHR hubs on one control. The most allowed is two, and an M1DBHR cannot sit on a branch of another or act as an extender.".into(), "plan");
    }
    r.main_bus_ft = main_ft;
    r.devices = all.len() as i32;
    if main_ft > MAX_BUS_FT {
        iss.err(
            format!(
                "The data bus is {} ft (hub home runs count double). The most allowed is {} ft.",
                f(main_ft),
                f(MAX_BUS_FT)
            ),
            "plan",
        );
    } else if main_ft > 0.8 * MAX_BUS_FT {
        iss.warn(
            format!(
                "The data bus is {} ft, close to the {} ft limit.",
                f(main_ft),
                f(MAX_BUS_FT)
            ),
            "plan",
        );
    }
    r.jp3 = runs.len() == 1;

    // ---- addresses: one use per device type
    for (d, w) in &all {
        match model(&d.model) {
            None => iss.err(format!("Unknown device model \"{}\".", d.model), w),
            Some(m) => {
                if d.address < m.min_address || d.address > m.max_address {
                    let short = m.label.split('(').next().unwrap_or("").trim();
                    iss.err(
                        format!(
                            "{}: {} addresses run from {} to {}.",
                            dev(d),
                            short,
                            m.min_address,
                            m.max_address
                        ),
                        w,
                    );
                }
            }
        }
    }
    // GroupBy keeps first-occurrence order of the keys
    let mut groups: Vec<((i32, i32), Vec<usize>)> = Vec::new();
    for (idx, (d, _)) in all.iter().enumerate() {
        if let Some(m) = model(&d.model) {
            let key = (m.type_, d.address);
            match groups.iter_mut().find(|g| g.0 == key) {
                Some(g) => g.1.push(idx),
                None => groups.push((key, vec![idx])),
            }
        }
    }
    for (key, members) in groups.iter().filter(|g| g.1.len() > 1) {
        let list = members
            .iter()
            .map(|&i| dev(all[i].0))
            .collect::<Vec<_>>()
            .join(", ");
        iss.err(
            format!("Address {} is used by {} Type {} devices: {}. Within a type every address must be different (a keypad and an M1XIN may share one, a keypad and an M1KAM may not).", key.1, members.len(), key.0, list),
            &all[members[0]].1,
        );
    }

    // ---- wireless transceivers and hardwired expanders
    let tw: Vec<&(&BusDevice, String)> = all
        .iter()
        .filter(|x| x.0.model.eq_ignore_ascii_case("M1XRFTWM"))
        .collect();
    if tw.len() > 4 {
        iss.err(
            format!(
                "{} M1XRFTWM transceivers. Up to 4 may be installed.",
                tw.len()
            ),
            "plan",
        );
    }
    if !tw.is_empty() && tw.iter().all(|x| x.0.address != 2) {
        iss.err("The primary M1XRFTWM transceiver must be at address 2; the additional ones use 3, 4 and 5.".into(), &tw[0].1);
    }
    let xin: Vec<&(&BusDevice, String)> = all
        .iter()
        .filter(|x| x.0.model.eq_ignore_ascii_case("M1XIN"))
        .collect();
    for (d, w) in xin
        .iter()
        .map(|x| (x.0, &x.1))
        .filter(|(d, _)| (2..=13).contains(&d.address))
    {
        let (z0, z1) = zones_at(d.address);
        if plan.wireless_groups.contains(&d.address) {
            iss.err(format!("{} uses zones {}-{}, which this account has set as wireless. Move the expander or change the wireless zone groups.", dev(d), z0, z1), w);
        }
    }
    if !tw.is_empty() {
        let lost = xin
            .iter()
            .filter(|x| (3..=10).contains(&x.0.address))
            .count() as i32;
        iss.info(
            format!("With wireless in use, each hardwired expander at address 3 to 10 (zones 33-160) uses up one group of 16 wireless zones. {} of those here: at most {} wireless zones remain (up to 144 without any).", lost, (9 - lost).max(0) * 16),
            "",
        );
    }

    // ---- power
    let mut ma: f64 = all.iter().fold(0.0, |acc, x| acc + x.0.ma.max(0.0)); // not .sum(): that starts at -0.0
    let mut active = 0;
    for m in runs.iter().filter(|m| m.kind.eq_ignore_ascii_case("dbhr")) {
        let br = m.branches.iter().filter(|b| !b.runs.is_empty()).count() as i32;
        active += br;
        ma += 30.0 + 20.0 * br as f64;
    }
    r.current_ma = ma;
    if all.iter().any(|x| x.0.ma > 0.0) || active > 0 {
        if ma > PTC_MA {
            iss.err(format!("The devices draw about {} mA. The control's +12V keypad output is protected by a 1.25 A fuse (PTC); use an auxiliary 12 V supply for part of the bus.", f(ma)), "plan");
        } else if ma > 0.8 * PTC_MA {
            iss.warn(format!("The devices draw about {} mA, near the 1.25 A the control's +12V keypad output can supply.", f(ma)), "plan");
        }
    } else if !all.is_empty() {
        iss.info("No device currents entered, so the power load was not checked. The +12V keypad output of the control is protected at 1.25 A; the hub adds 30 mA, plus 20 mA per branch for the M1DBHR.".into(), "");
    }

    // ---- advice
    if runs.iter().any(|x| x.kind.eq_ignore_ascii_case("direct")) && all.len() >= 4 {
        iss.info("A data bus hub is the preferred way to wire a bus with several devices (new wiring: M1DBH with CAT5/6 home runs; reusing 4-conductor cable: M1DBHR). A direct bus is limited to two runs.".into(), "");
    }
    r.issues = iss.0;
    build_checklist(plan, &mut r, &all);
    r
}

fn last_dev(run: &BusRun) -> Option<&BusDevice> {
    run.devices.last()
}

fn build_checklist(plan: &BusPlan, r: &mut PlanResult, all: &Placed) {
    let runs = &plan.runs;
    let ctrl = vec![
        if runs.len() == 1 {
            "JP3 (terminating jumper on the M1 control): INSTALL it. With one run, the control end is the second of the two termination points.".to_string()
        } else {
            "JP3 (terminating jumper on the M1 control): REMOVE it. With two runs, the far end of each run is terminated instead.".to_string()
        },
        "The data bus must have exactly two terminating points in total, never more.".to_string(),
    ];
    r.checklist.push(ChecklistGroup {
        title: "Control".into(),
        items: ctrl,
    });
    for (i, m) in runs.iter().enumerate() {
        let mut items: Vec<String> = Vec::new();
        let title = format!(
            "Run {}{}",
            i + 1,
            if m.name.trim().is_empty() {
                String::new()
            } else {
                format!(": {}", m.name.trim())
            }
        );
        match m.kind.to_lowercase().as_str() {
            "direct" => {
                let chain = if m.run.devices.is_empty() {
                    "(no devices)".to_string()
                } else {
                    m.run
                        .devices
                        .iter()
                        .map(dev)
                        .collect::<Vec<_>>()
                        .join(" \u{2192} ")
                };
                items.push(format!(
                    "Daisy chain: control \u{2192} {}. About {} ft.",
                    chain,
                    f(m.run.length_ft)
                ));
                if let Some(d) = last_dev(&m.run) {
                    items.push(format!("Terminate the last device only: {}. Fit its terminating jumper. Do not terminate the others.", dev(d)));
                }
            }
            "dbh" => {
                items.push(format!("Connect hub 1 to the control's +12V, DATA A, DATA B and NEG terminals with a dedicated 4-wire cable, 22 AWG minimum ({} ft). The optional ELK-W018B ribbon cable can be used if the hub is under 24 inches from the board.", f(m.feed_ft)));
                for (h, hub) in m.hubs.iter().enumerate() {
                    let last = h == m.hubs.len() - 1;
                    for (j, hr) in hub.home_runs.iter().enumerate() {
                        let to = if hr.devices.is_empty() {
                            "(no device)".to_string()
                        } else {
                            hr.devices.iter().map(dev).collect::<Vec<_>>().join(" + ")
                        };
                        items.push(format!(
                            "Hub {}, J{}: home run \"{}\" ({} ft) to {}.",
                            h + 1,
                            j + 1,
                            hr.name,
                            f(hr.length_ft),
                            to
                        ));
                    }
                    let free = hub.home_runs.len() + 1;
                    if hub.ends_at_device(last) {
                        let lr = &hub.home_runs[hub.home_runs.len() - 1];
                        let ld = match lr.devices.last() {
                            Some(d) => dev(d),
                            None => "the device on the last jack".to_string(),
                        };
                        items.push(format!("Hub {}: no terminating plug. Fit the terminating jumper on the device on the last jack (J{}): {}. It is the only termination on the hub side; terminate no other device.", h + 1, hub.home_runs.len(), ld));
                    } else if last {
                        items.push(format!("Hub {}, J{}: insert the RJ45 terminating plug (120 \u{03A9}, supplied). It is the only termination on the hub side. Do not terminate any device.", h + 1, free));
                    } else {
                        items.push(format!("Hub {}, J{}: crossover cable (orange and green pairs swapped) to J1 of hub {}. No terminating plug on this hub.", h + 1, free, h + 2));
                    }
                }
                items.push("Fill jacks from J1 upward without skipping one. Use continuous CAT5/6 from hub to device, no splices and no other cable type. Wire to the device terminals with the T568A colours (below).".into());
            }
            "dbhr" => {
                items.push(format!("Mount the M1DBHR inside the M1 enclosure. Connect its +, A, B and \u{2212} terminals to the control's +VKP, DATA A, DATA B and NEG with a short 4-conductor cable ({} ft). If the hub is powered from an auxiliary supply, do not connect +VKP and join all the negatives.", f(m.feed_ft)));
                items.push("JP1 (terminates the input from the control): INSTALL it.".into());
                for (b, br) in m.branches.iter().enumerate() {
                    let jp = format!("JP{}", b + 2);
                    if br.runs.is_empty() {
                        items.push(format!(
                            "Branch {}: not used. Leave {} as it is; no devices are connected.",
                            b + 1,
                            jp
                        ));
                        continue;
                    }
                    let parts: Vec<String> = br
                        .runs
                        .iter()
                        .map(|hr| {
                            format!(
                                "home run \"{}\" ({} ft): {}",
                                hr.name,
                                f(hr.length_ft),
                                if hr.devices.is_empty() {
                                    "(no devices)".to_string()
                                } else {
                                    hr.devices
                                        .iter()
                                        .map(dev)
                                        .collect::<Vec<_>>()
                                        .join(" \u{2192} ")
                                }
                            )
                        })
                        .collect();
                    items.push(format!("Branch {}: {}.", b + 1, parts.join("; ")));
                    items.push(if br.runs.len() == 1 {
                        format!(
                            "Branch {}: terminate the last device ({}) AND fit {} on the hub. That is the branch's two terminations.",
                            b + 1,
                            match last_dev(&br.runs[0]) {
                                Some(d) => dev(d),
                                None => "?".to_string(),
                            },
                            jp
                        )
                    } else {
                        format!("Branch {}: terminate the last device of each home run and REMOVE {}.", b + 1, jp)
                    });
                }
            }
            _ => {}
        }
        r.checklist.push(ChecklistGroup { title, items });
    }

    // address / switch list, ordered by type then address (stable)
    let mut known: Vec<(&BusDevice, &'static BusModel)> = all
        .iter()
        .filter_map(|(d, _)| model(&d.model).map(|m| (*d, m)))
        .collect();
    known.sort_by_key(|(d, m)| (m.type_, d.address));
    let mut sw: Vec<String> = Vec::new();
    for (d, m) in known {
        let mut extra = String::new();
        if m.id == "M1XIN" {
            if let Some(s1) = lookup(&ZONE_EXPANDER_SWITCHES, d.address) {
                let (a, b) = zones_at(d.address);
                extra = format!(": zones {}-{}; {}", a, b, s1);
            }
        } else if m.id == "M1XRFTWM" {
            if let Some(s2) = lookup(&TRANSCEIVER_SWITCHES, d.address) {
                extra = format!(
                    ": {} transceiver; {}",
                    if d.address == 2 {
                        "primary"
                    } else {
                        "additional"
                    },
                    s2
                );
            }
        }
        sw.push(format!("{}{}", dev(d), extra));
    }
    if !sw.is_empty() {
        sw.push("After setting the addresses, run Bus Module Enrollment (installer programming, or Live > Modules in this editor). Devices added later must be enrolled too.".into());
        r.checklist.push(ChecklistGroup {
            title: "Addresses to set".into(),
            items: sw,
        });
    }
    // note: the C# compares the kind case-sensitively here (`x.Kind is "dbh"`)
    if runs.iter().any(|x| x.kind == "dbh") {
        r.checklist.push(ChecklistGroup {
            title: "CAT5/6 colours (T568A, hub to device)".into(),
            items: vec![
                "Brown = +12V, white/brown = NEG (each connects on its own).".into(),
                "Orange = DATA A and green = A1 go together under the device's A terminal.".into(),
                "White/orange = DATA B and white/green = B1 go together under the B terminal."
                    .into(),
                "The blue pair is unused.".into(),
            ],
        });
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// JSON input (System.Text.Json web defaults: case-insensitive names, numbers may be strings, unknown members ignored)
// ---------------------------------------------------------------------------------------------------------------------------------

fn member<'a>(m: &'a Map<String, Value>, name: &str) -> Option<&'a Value> {
    m.iter()
        .rev()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v)
        .filter(|v| !v.is_null())
}

fn as_object<'a>(v: &'a Value, what: &str) -> Result<&'a Map<String, Value>, String> {
    v.as_object()
        .ok_or_else(|| format!("The JSON value could not be converted to {}.", what))
}

fn get_str(m: &Map<String, Value>, name: &str, default: &str) -> Result<String, String> {
    match member(m, name) {
        None => Ok(default.to_string()),
        Some(Value::String(s)) => Ok(s.clone()),
        Some(_) => Err(format!(
            "The JSON value could not be converted to System.String. Path: $.{}",
            name
        )),
    }
}

fn get_f64(m: &Map<String, Value>, name: &str, default: f64) -> Result<f64, String> {
    let bad = || {
        format!(
            "The JSON value could not be converted to System.Double. Path: $.{}",
            name
        )
    };
    match member(m, name) {
        None => Ok(default),
        Some(Value::Number(n)) => n.as_f64().ok_or_else(bad),
        Some(Value::String(s)) => s
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or_else(bad),
        Some(_) => Err(bad()),
    }
}

fn value_i32(v: &Value, name: &str) -> Result<i32, String> {
    let bad = || {
        format!(
            "The JSON value could not be converted to System.Int32. Path: $.{}",
            name
        )
    };
    match v {
        Value::Number(n) => n
            .as_i64()
            .and_then(|i| i32::try_from(i).ok())
            .ok_or_else(bad),
        Value::String(s) => s.trim().parse::<i32>().map_err(|_| bad()),
        _ => Err(bad()),
    }
}

fn get_i32(m: &Map<String, Value>, name: &str, default: i32) -> Result<i32, String> {
    match member(m, name) {
        None => Ok(default),
        Some(v) => value_i32(v, name),
    }
}

fn get_bool(m: &Map<String, Value>, name: &str) -> Result<bool, String> {
    match member(m, name) {
        None => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(format!(
            "The JSON value could not be converted to System.Boolean. Path: $.{}",
            name
        )),
    }
}

fn get_list<'a>(m: &'a Map<String, Value>, name: &str) -> Result<&'a [Value], String> {
    match member(m, name) {
        None => Ok(&[]),
        Some(Value::Array(a)) => Ok(a),
        Some(_) => Err(format!(
            "The JSON value could not be converted to a list. Path: $.{}",
            name
        )),
    }
}

fn parse_device(v: &Value) -> Result<BusDevice, String> {
    if v.is_null() {
        return Ok(BusDevice::default());
    }
    let m = as_object(v, "BusDevice")?;
    let d = BusDevice::default();
    Ok(BusDevice {
        model: get_str(m, "model", &d.model)?,
        address: get_i32(m, "address", d.address)?,
        name: get_str(m, "name", "")?,
        ma: get_f64(m, "ma", 0.0)?,
    })
}

fn parse_run(v: &Value) -> Result<BusRun, String> {
    if v.is_null() {
        return Ok(BusRun::default());
    }
    let m = as_object(v, "BusRun")?;
    Ok(BusRun {
        name: get_str(m, "name", "")?,
        length_ft: get_f64(m, "lengthFt", 0.0)?,
        devices: get_list(m, "devices")?
            .iter()
            .map(parse_device)
            .collect::<Result<_, _>>()?,
    })
}

fn parse_hub(v: &Value) -> Result<BusHub, String> {
    if v.is_null() {
        return Ok(BusHub::default());
    }
    let m = as_object(v, "BusHub")?;
    Ok(BusHub {
        home_runs: get_list(m, "homeRuns")?
            .iter()
            .map(parse_run)
            .collect::<Result<_, _>>()?,
        crossover_ft: get_f64(m, "crossoverFt", 0.0)?,
        terminate_at_device: get_bool(m, "terminateAtDevice")?,
    })
}

fn parse_branch(v: &Value) -> Result<BusBranch, String> {
    if v.is_null() {
        return Ok(BusBranch::default());
    }
    let m = as_object(v, "BusBranch")?;
    Ok(BusBranch {
        runs: get_list(m, "runs")?
            .iter()
            .map(parse_run)
            .collect::<Result<_, _>>()?,
    })
}

fn parse_main_run(v: &Value) -> Result<MainRun, String> {
    if v.is_null() {
        return Ok(MainRun::default());
    }
    let m = as_object(v, "MainRun")?;
    let d = MainRun::default();
    Ok(MainRun {
        kind: get_str(m, "kind", &d.kind)?,
        name: get_str(m, "name", "")?,
        run: match member(m, "run") {
            None => BusRun::default(),
            Some(r) => parse_run(r)?,
        },
        feed_ft: get_f64(m, "feedFt", d.feed_ft)?,
        hubs: get_list(m, "hubs")?
            .iter()
            .map(parse_hub)
            .collect::<Result<_, _>>()?,
        branches: get_list(m, "branches")?
            .iter()
            .map(parse_branch)
            .collect::<Result<_, _>>()?,
    })
}

impl BusPlan {
    /// Deserialises like `JsonSerializer.DeserializeAsync<BusPlan>(body, web defaults)`. `null` gives an empty plan.
    /// Error text is not identical to System.Text.Json's (the C# returns it as the 400 body).
    pub fn from_json(text: &str) -> Result<BusPlan, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if v.is_null() {
            return Ok(BusPlan::default());
        }
        let m = as_object(&v, "BusPlan")?;
        let wireless = get_list(m, "wirelessGroups")?
            .iter()
            .map(|x| value_i32(x, "wirelessGroups"))
            .collect::<Result<_, _>>()?;
        Ok(BusPlan {
            runs: get_list(m, "runs")?
                .iter()
                .map(parse_main_run)
                .collect::<Result<_, _>>()?,
            wireless_groups: wireless,
        })
    }
}

/// `POST /api/databus/check`: `Ok(body)` is the 200 response, `Err(message)` the 400 response body.
pub fn check_json(body: &str) -> Result<String, String> {
    Ok(check(&BusPlan::from_json(body)?).to_json())
}

#[cfg(test)]
mod tests;
