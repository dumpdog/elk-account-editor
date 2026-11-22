//! ElkRP's English wording of rules (`english.rs`) and the validation of new or changed rules (`rules::validate_rule`).
//! The wording is a port of ElkRP's ConstructEnglishTrigger / Condition / Result (decompiled ElkRP 5.3, M1G), so the expected
//! strings below were derived from that code (and, for the sample account, checked against what the code gives for its six rules).
//! No oracle exists for creating rules; the round-trip tests build a rule, save, reopen and compare the text.

use std::fs;
use std::path::PathBuf;

use elk_account::english::{english_report, file_rule_lines, lines_of, percent_used, Ctx, Names};
use elk_account::*;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn sample() -> AccountFile {
    let b = fs::read(root().join("sample/ElkRPAcctSample Export")).unwrap();
    AccountFile::parse(&decode_latin1(&b)).unwrap()
}

fn lines(p: &[[u8; 8]], f: &AccountFile) -> Vec<String> {
    let names = Names::from_file(f);
    let cx = Ctx {
        a: f.automation.as_ref().unwrap(),
        n: &names,
    };
    lines_of(&cx, p)
}

fn one(p: [u8; 8], f: &AccountFile) -> String {
    lines(&[p], f).join(" / ")
}

#[test]
fn sample_account_rules_read_like_elkrp() {
    // needs the event table (ElkRP wording), which the public source tree does not include
    if !elk_account::rules::event_table_present() {
        return;
    }
    let f = sample();
    let got: Vec<String> = file_rule_lines(&f)
        .unwrap()
        .iter()
        .map(|l| l.join(" / "))
        .collect();
    assert_eq!(
        got,
        vec![
            "WHENEVER ANY BURGLAR ALARM, ANY AREA TURNS ON / THEN TURN Strobe Light (Out 3) ON / THEN TURN Flood Lights [3 (A3)] ON",
            "WHENEVER Smith Household (Area 1) ARM STATE BECOMES DISARMED / THEN TURN Strobe Light (Out 3) OFF",
            "WHENEVER THE TIME IS 6:00 AM / AND THE MONTH IS June / AND THE DAY OF THE MONTH IS 10 / THEN DISPLAY \"Happy Birthday  Mr. Smith\" IN Smith Household (Area 1) INDEFINITELY, [*] CLEARS",
            "WHENEVER Smith Household (Area 1) ENTRY DELAY STARTS / AND IT IS DARK OUTSIDE / THEN TURN Foyer Lights [2 (A2)] ON",
            "WHENEVER Smith Household (Area 1) IS ARMED AWAY / AND THE TIME IS AT SUNSET / THEN TURN Front Porch [1 (A1)] ON",
            "WHENEVER THE TIME IS 11:00 PM / THEN TURN Front Porch [1 (A1)] OFF / THEN TURN Foyer Lights [2 (A2)] OFF",
        ]
    );
}

#[test]
fn zone_task_output_wording_matches_the_elkrp_screen() {
    // needs the event table (ElkRP wording), which the public source tree does not include
    if !elk_account::rules::event_table_present() {
        return;
    }
    // the lines ElkRP's Rules page shows for a gate controller, with made-up names
    let mut f = sample();
    {
        let a = f.automation.as_mut().unwrap();
        let name = |r: &mut Vec<u8>, at: usize, s: &str| {
            for (i, c) in s.bytes().enumerate() {
                r[at + i] = c;
            }
        };
        a.tasks[0][..16].fill(0);
        name(&mut a.tasks[0], 0, "Open Gate");
        for i in [16, 17, 2] {
            a.outputs[i][..16].fill(0);
        }
        name(&mut a.outputs[16], 0, "Gate Open");
        name(&mut a.outputs[17], 0, "Door Open");
        name(&mut a.outputs[2], 0, "Siren Power");
    }
    // zone 88: pretend it exists as a normal wired zone
    let zn = f.zones.entries[0].clone();
    let mut z = zn.clone();
    z.number = 88;
    z.bytes[4..20].copy_from_slice(b"Gate Open       ");
    f.zones.entries.push(z);
    assert_eq!(
        one([97, 2, 0, 88, 26, 17, 0, 0], &f),
        "WHENEVER Gate Open (Zn 88) BECOMES NOT SECURE / THEN TURN Gate Open (Out 17) ON"
    );
    assert_eq!(
        one([89, 1, 7, 209, 26, 17, 0, 3], &f),
        "WHENEVER Open Gate (Task 1)  IS ACTIVATED / THEN TURN Gate Open (Out 17) ON FOR 3 SECS"
    );
    assert_eq!(
        one([89, 1, 3, 233, 80, 2, 0, 0], &f),
        "WHENEVER ANY FIRE ALARM TURNS ON / THEN ACTIVATE Go to Work (Task 2)"
    );
    assert_eq!(
        one([89, 1, 7, 212, 27, 18, 0, 10], &f),
        "WHENEVER Task 04 (Task 4)  IS ACTIVATED / THEN TURN Door Open (Out 18) ON FOR 10 SECS, RESTART TIMER IF RUNNING"
    );
    assert_eq!(
        one([89, 0, 3, 246, 24, 3, 0, 0], &f),
        "WHENEVER ANY ALARM, ANY AREA CUTS OFF / THEN TURN Siren Power (Out 3) OFF"
    );
    // toggle, no time
    assert_eq!(
        one([97, 0, 0, 88, 28, 17, 0, 0], &f),
        "WHENEVER Gate Open (Zn 88) BECOMES SECURE / THEN TOGGLE Gate Open (Out 17)"
    );
}

#[test]
fn triggers_and_conditions() {
    let f = sample();
    let t = |b: [u8; 8]| one(b, &f);
    assert_eq!(
        t([17, 0, 13, 5, 80, 1, 0, 0]),
        "WHENEVER THE TIME IS 1:05 PM / THEN ACTIVATE Welcome Home (Task 1)"
    );
    assert_eq!(
        t([23, 0, 2, 30, 80, 1, 0, 0]),
        "WHENEVER EVERY 2 HOURS AND 30 MINUTES / THEN ACTIVATE Welcome Home (Task 1)"
    );
    assert_eq!(
        t([160, 0, 0, 45, 80, 1, 0, 0]),
        "WHENEVER EVERY 45 SECONDS / THEN ACTIVATE Welcome Home (Task 1)"
    );
    assert_eq!(
        t([81, 0, 0, 0, 80, 1, 0, 0]),
        "WHENEVER SUNSET / THEN ACTIVATE Welcome Home (Task 1)"
    );
    assert_eq!(
        t([73, 0, 255, 196, 80, 1, 0, 0]),
        "WHENEVER THE TIME IS 1 HOUR BEFORE SUNRISE / THEN ACTIVATE Welcome Home (Task 1)"
    );
    assert_eq!(
        t([81, 0, 0, 90, 80, 1, 0, 0]),
        "WHENEVER THE TIME IS 1 HOUR AND 30 MINUTES AFTER SUNSET / THEN ACTIVATE Welcome Home (Task 1)"
    );
    assert!(t([25, 17, 0, 0, 80, 1, 0, 0]).starts_with("WHENEVER THE HOUR IS 5:xx  PM"));
    assert_eq!(t([128 + 3, 4, 0, 9, 80, 1, 0, 0]), "WHENEVER Counter04 (Counter 4) CHANGES TO A VALUE GREATER THAN 9 / THEN ACTIVATE Welcome Home (Task 1)");
    // lighting trigger
    assert!(t([115, 0, 0, 0, 80, 1, 0, 0]).starts_with("WHENEVER Front Porch [1 (A1)] "));
    // conditions
    let p = |c: [u8; 8]| lines(&[[17, 0, 6, 0, 8, 0, 0, 0], c], &f)[1].clone();
    assert_eq!(
        p([41, 62, 0, 0, 80, 1, 0, 0]),
        "AND THE DAY(S) OF THE WEEK IS/ARE -MTWTF-"
    );
    assert_eq!(
        p([48 + 2, 7, 0, 0, 80, 1, 0, 0]),
        "AND THE MONTH IS NOT July"
    );
    assert_eq!(
        p([16 + 3, 0, 22, 0, 80, 1, 0, 0]),
        "AND TIME OF DAY IS LATER THAN 10:00 PM"
    );
    assert_eq!(
        p([64 + 4, 26, 0, 0, 80, 1, 0, 0]),
        "AND THE YEAR IS EARLIER THAN 2026"
    );
    assert_eq!(
        p([97, 4, 0, 1, 80, 1, 0, 0]),
        "AND Front Entry Door (Zn 1) IS NORMAL"
    );
}

#[test]
fn results() {
    let f = sample();
    let r = |b: [u8; 4]| {
        let mut p = [17, 0, 6, 0, 0, 0, 0, 0];
        p[4..].copy_from_slice(&b);
        lines(&[p], &f)[1].clone()
    };
    assert_eq!(
        r([16 | 2, 3, 0, 90]),
        "THEN ARM AREA(S) 1, 2 TO STAY AFTER 1 MIN"
    );
    assert_eq!(r([16, 1, 0, 0]), "THEN DISARM AREA(S) 1 IMMEDIATELY");
    assert_eq!(r([72 | 1, 5, 0, 7]), "THEN ADD 7 TO Counter05 (Counter 5)");
    assert_eq!(r([72, 5, 0, 7]), "THEN SET Counter05 (Counter 5) TO 7");
    assert_eq!(
        r([104 | 3, 0, 0, 0]),
        "THEN CHIRP THE OUTSIDE SIREN 3 TIMES"
    );
    assert_eq!(
        r([96, 1, 0, 0]),
        "THEN START KEYPAD BEEPS IN Smith Household (Area 1)"
    );
    assert_eq!(
        r([136, 3, 0, 0]),
        "THEN SET CHIME TO DING AND VOICE IN Smith Household (Area 1)"
    );
    assert_eq!(
        r([120, 0, 0, 90]),
        "THEN EXPECT Smith Household (Area 1) TO ARM WITHIN 2 MINUTES"
    );
    assert_eq!(
        r([224, 1, 0, 0]),
        "THEN BYPASS ALL BURGLAR ZONES IN Smith Household (Area 1)"
    );
    assert_eq!(
        r([224, 0, 0x03, 0xE7]),
        "THEN UNBYPASS ALL NON-SECURE BURGLAR ZONES IN Smith Household (Area 1)"
    );
    assert_eq!(r([176, 2, 1, 0]), "THEN SET Thermostat01 (Tstat 1) TO COOL");
    assert_eq!(
        r([176, 72, 1, 8]),
        "THEN SET Thermostat01 (Tstat 1) COOLING DESIRED TEMP TO 72 DEG. F. (22 DEG. C.)"
    );
    assert_eq!(r([208, 1, 0, 0]), "THEN DISABLE NON-ALARM VOICE MESSAGES");
    assert_eq!(r([24, 255, 0, 0]), "THEN RESET SMOKE POWER");
    // an output for a custom value or a long time
    assert_eq!(
        r([26, 3, 0, 120]),
        format!("THEN TURN {} ON FOR 2 MINS", out_name(&f, 3))
    );
}

fn out_name(f: &AccountFile, n: usize) -> String {
    let a = f.automation.as_ref().unwrap();
    let t = elk_account::automation::name_at(&a.outputs[n - 1], 0, 16);
    if t.is_empty() {
        format!("Output {n}")
    } else {
        format!("{t} (Out {n})")
    }
}

#[test]
fn lighting_with_a_for_packet() {
    let f = sample();
    // THEN turn light 1 on FOR 90 seconds (FOR packet) and FOR a custom value
    let then = [17, 0, 6, 0, 32, 0, 3, 0];
    let for_secs = [152, 0, 0, 0, 0, 0, 0, 90];
    assert_eq!(
        lines(&[then, for_secs], &f),
        vec![
            "WHENEVER THE TIME IS 6:00 AM".to_string(),
            "THEN TURN Front Porch [1 (A1)] ON FOR 1 MIN, 30 SECS".to_string()
        ]
    );
    let for_cust = [152, 13, 0, 0, 0, 0, 0, 0];
    assert!(lines(&[then, for_cust], &f)[1].starts_with("THEN TURN Front Porch [1 (A1)] ON FOR "));
}

#[test]
fn space_footer_rounds_like_vb() {
    assert_eq!(percent_used(12, 0), 2);
    assert_eq!(percent_used(264, 0), 50);
    assert_eq!(percent_used(0, 0), 0);
    assert_eq!(percent_used(528, 0), 100);
    assert_eq!(percent_used(1, 0), 0);
}

#[test]
fn report_has_the_budget() {
    let f = sample();
    let r = english_report(&f).unwrap();
    assert_eq!(r.rules.len(), 6);
    assert_eq!(r.rule_packets_used, 12);
    assert_eq!(r.percent, 3);
}

// ---- creating rules: validation and round trip ------------------------------------------------------------------------------

fn model_with(rules: Vec<Vec<[u8; 8]>>) -> (AccountFile, AccountModel) {
    let f = sample();
    let mut m = AccountModel::from(&f).unwrap();
    let a = m.automation.as_mut().unwrap();
    a.rules = rules
        .into_iter()
        .enumerate()
        .map(|(i, pk)| RuleDto {
            n: i as i32 + 1,
            comment: String::new(),
            packets: pk
                .iter()
                .map(|p| p.iter().map(|&b| b as i32).collect())
                .collect(),
            summary: String::new(),
            ..Default::default()
        })
        .collect();
    (f, m)
}

fn save(rules: Vec<Vec<[u8; 8]>>) -> std::result::Result<AccountFile, String> {
    let (mut f, m) = model_with(rules);
    m.apply_to(&mut f).map_err(|e| e.message().to_string())?;
    Ok(f)
}

#[test]
fn a_new_rule_is_written_and_reads_back_the_same() {
    // needs the event table (ElkRP wording), which the public source tree does not include
    if !elk_account::rules::event_table_present() {
        return;
    }
    let rules = vec![
        vec![[97, 2, 0, 1, 26, 5, 0, 3]],
        vec![
            [89, 1, 7, 209, 8, 0, 0, 0],
            [16 + 3, 0, 22, 0, 32, 1, 3, 0],
            [152, 0, 0, 0, 0, 0, 2, 88],
        ],
        vec![[17, 0, 6, 0, 80, 2, 0, 0], [144, 0, 0, 0, 72, 3, 0, 5]],
    ];
    let f = save(rules.clone()).unwrap();
    let want = file_rule_lines(&f).unwrap();
    let again = AccountFile::parse(&f.to_text()).unwrap();
    assert_eq!(file_rule_lines(&again).unwrap(), want);
    assert_eq!(want.len(), 3);
    assert_eq!(
        want[1],
        vec![
            "WHENEVER Welcome Home (Task 1)  IS ACTIVATED".to_string(),
            "AND TIME OF DAY IS LATER THAN 10:00 PM".to_string(),
            "THEN TURN Foyer Lights [2 (A2)] ON FOR 10 MINS".to_string()
        ]
    );
    // only the rule area changed
    let orig = sample().to_text();
    let new = f.to_text();
    let (a, b): (Vec<&str>, Vec<&str>) = (orig.lines().collect(), new.lines().collect());
    assert_eq!(a.len(), b.len());
    let diff: Vec<usize> = (0..a.len()).filter(|&i| a[i] != b[i]).collect();
    let first = diff[0];
    let last = *diff.last().unwrap();
    assert!(
        last - first < 8 * 528 + 600,
        "changes stay inside the rule block: {first}..{last}"
    );
}

#[test]
fn bad_rules_are_refused_with_a_reason() {
    // needs the event table (ElkRP wording), which the public source tree does not include
    if !elk_account::rules::event_table_present() {
        return;
    }
    let err = |p: Vec<[u8; 8]>| save(vec![p]).unwrap_err();
    // the first packet is not a trigger
    assert!(err(vec![[144, 0, 0, 0, 24, 1, 0, 0]]).contains("not a WHENEVER trigger"));
    // an AND without a THEN
    assert!(err(vec![[97, 2, 0, 1, 8, 0, 0, 0]]).contains("AND is not followed by a THEN"));
    // no THEN at all
    assert!(err(vec![[97, 2, 0, 1, 0, 0, 0, 0]]).contains("no valid THEN"));
    // zone 0 and 209
    assert!(err(vec![[97, 2, 0, 0, 24, 1, 0, 0]]).contains("zone must be 1-208"));
    assert!(err(vec![[97, 2, 0, 209, 24, 1, 0, 0]]).contains("zone must be 1-208"));
    // output 65, task 33
    assert!(err(vec![[97, 2, 0, 1, 24, 65, 0, 0]]).contains("output must be 1-64"));
    assert!(err(vec![[97, 2, 0, 1, 80, 33, 0, 0]]).contains("task must be 1-32"));
    // minute 60, hour 24
    assert!(err(vec![[17, 0, 24, 0, 80, 1, 0, 0]]).contains("hour must be 0-23"));
    assert!(err(vec![[17, 0, 1, 60, 80, 1, 0, 0]]).contains("minute must be 0-59"));
    // arm no areas
    assert!(err(vec![[97, 2, 0, 1, 17, 0, 0, 0]]).contains("at least one area"));
    // an event that does not exist
    assert!(err(vec![[89, 1, 0x27, 0x0F, 24, 1, 0, 0]]).contains("not in ElkRP's event list"));
    // a FOR after something that has no FOR, and an extra THEN that is not type 18
    assert!(
        err(vec![[97, 2, 0, 1, 80, 1, 0, 0], [152, 0, 0, 0, 0, 0, 0, 5]])
            .contains("only belongs to a lighting")
    );
    assert!(
        err(vec![[97, 2, 0, 1, 80, 1, 0, 0], [97, 2, 0, 1, 80, 1, 0, 0]])
            .contains("additional THEN")
    );
    // a FOR without time
    assert!(
        err(vec![[97, 2, 0, 1, 32, 0, 3, 0], [152, 0, 0, 0, 0, 0, 0, 0]])
            .contains("FOR needs a time")
    );
    // a text pointer to nowhere
    assert!(
        err(vec![[97, 2, 0, 1, 56, 0, 0, 5]]).contains("text string it points at does not exist")
            || err(vec![[97, 2, 0, 1, 56, 0, 0, 5]]).contains("valid THEN")
    );
}

#[test]
fn old_rules_of_odd_kinds_do_not_block_a_save() {
    // the file's own six rules are saved again untouched; and a reorder is fine
    let f = sample();
    let m = AccountModel::from(&f).unwrap();
    let mut f2 = f.clone();
    m.apply_to(&mut f2).unwrap();
    assert_eq!(f2.to_text(), f.to_text());
}

#[test]
fn too_many_packets_still_refused() {
    let rule: Vec<[u8; 8]> = vec![[97, 2, 0, 1, 26, 5, 0, 3]];
    let e = save(vec![rule; 529]).unwrap_err();
    assert!(e.contains("packets"), "{e}");
}

#[test]
fn local_real_export_reads_like_elkrp_when_present() {
    // needs the event table (ElkRP wording), which the public source tree does not include
    if !elk_account::rules::event_table_present() {
        return;
    }
    // TESTING\ is git-ignored (real account): only the shape of each line is checked here, never names
    let p = root().join("TESTING/ElkRPAcct.exp");
    let Ok(b) = fs::read(p) else { return };
    let f = AccountFile::parse(&decode_latin1(&b)).unwrap();
    let got: Vec<String> = file_rule_lines(&f)
        .unwrap()
        .iter()
        .map(|l| l.join(" / "))
        .collect();
    assert_eq!(got.len(), 10);
    let tails = [
        "TURNS ON / THEN ACTIVATE ",
        "BECOMES NOT SECURE / THEN TURN ",
        "BECOMES NOT SECURE / THEN TURN ",
        "BECOMES NOT SECURE / THEN ACTIVATE ",
        "IS ACTIVATED / THEN TURN ",
        "IS ACTIVATED / THEN TURN ",
        "IS ACTIVATED / THEN TURN ",
        "IS ACTIVATED / THEN TURN ",
        "ANY ALARM, ANY AREA TURNS ON / THEN TURN ",
        "ANY ALARM, ANY AREA CUTS OFF / THEN TURN ",
    ];
    for (g, t) in got.iter().zip(tails) {
        assert!(g.starts_with("WHENEVER ") && g.contains(t), "{g}");
    }
    assert!(got[4].ends_with(" ON FOR 3 SECS"));
    assert!(got[7].ends_with(" ON FOR 10 SECS, RESTART TIMER IF RUNNING"));
}
