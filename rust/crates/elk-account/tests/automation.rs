//! Ports of the 13 tests of `port/tests/Elk.Tests/AutomationTests.cs` (tasks, counters, custom values, outputs, lights, voice
//! messages, rules and texts). Tests that need the git-ignored TESTING exports skip when they are absent.

#![allow(clippy::type_complexity)]

use std::fs;
use std::path::PathBuf;

use elk_account::*;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn read(p: &std::path::Path) -> String {
    decode_latin1(&fs::read(p).unwrap())
}

fn sample() -> String {
    read(&root().join("sample/ElkRPAcctSample Export"))
}

fn open(text: &str) -> (AccountFile, AccountModel) {
    let f = AccountFile::parse(text).unwrap();
    let m = AccountModel::from(&f).unwrap();
    (f, m)
}

fn reload(f: &AccountFile) -> AutomationDto {
    AccountModel::from(&AccountFile::parse(&f.to_text()).unwrap())
        .unwrap()
        .automation
        .unwrap()
}

fn auto(m: &mut AccountModel) -> &mut AutomationDto {
    m.automation.as_mut().unwrap()
}

fn is_format_err(r: Result<()>) -> bool {
    matches!(r, Err(Error::Format(_)))
}

#[test]
fn every_export_round_trips_byte_identical_and_automation_apply_changes_nothing() {
    let mut paths: Vec<PathBuf> = fs::read_dir(root().join("sample"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_file() && fs::metadata(p).unwrap().len() >= 60000)
        .collect();
    paths.push(root().join("port/src/Elk.Editor/template-m1g.txt"));
    let mut with_block = 0;
    for path in paths {
        let text = read(&path);
        let Ok(mut f) = AccountFile::parse(&text) else {
            continue;
        };
        assert!(f.to_text() == text, "unedited save differs: {path:?}");
        let m = AccountModel::from(&f).unwrap();
        m.apply_to(&mut f).unwrap();
        assert!(
            f.to_text() == text,
            "apply of the unedited model changed the file: {path:?}"
        );
        if f.automation.is_some() {
            with_block += 1;
        }
    }
    assert!(
        with_block >= 5,
        "expected the automation block in the sample exports"
    );
}

#[test]
fn sample_account_decodes_as_elkrp_shows_it() {
    let (_, m) = open(&sample());
    let a = m.automation.unwrap();
    assert_eq!(a.rule_packets_used, 12);
    assert_eq!(a.text_packets_used, 4);
    assert_eq!(a.packets_free, 512);
    assert_eq!(a.tasks[0].name, "Welcome Home");
    assert!(!a.tasks[0].show);
    assert_eq!(a.tasks[1].name, "Go to Work");
    assert!(a.tasks[1].show && a.tasks[2].show);
    assert_eq!(a.counters[0].as_deref(), Some("Counter01"));
    assert_eq!(a.thermostats[0].as_deref(), Some("Thermostat01"));
    assert_eq!(a.customs[0].value, 600);
    assert_eq!(a.customs[0].name, "BurgDuration");
    assert_eq!(a.customs[12].name, "YardSpklrTm");
    assert_eq!(a.customs[12].value, 10);
    assert_eq!(a.customs[12].type_, 0);
    assert_eq!(a.outputs[0].name, "Output 001");
    assert_eq!(a.lights[0].id, "A1");
    assert_eq!(a.lights[0].name, "Front Porch");
    assert!(a.lights[0].show);
    assert_eq!(a.lights[0].type_, 1);
    assert_eq!(a.lights[1].name, "Foyer Lights");
    assert_eq!(a.lights[1].type_, 2);
    assert_eq!(a.lights[255].id, "P16");
    assert_eq!(a.voice_messages[0].words, vec![51, 51, 250, 65534, 51, 51]);
    assert_eq!(a.texts.len(), 1);
    assert_eq!(a.texts[0].text, "Happy Birthday  Mr. Smith");
    assert_eq!(a.texts[0].used_by_rules, 1);
    assert_eq!(a.rules.len(), 6);
    assert_eq!(
        a.rules.iter().map(|r| r.packets.len()).collect::<Vec<_>>(),
        vec![2, 1, 3, 2, 2, 2]
    );
    // the wording of event triggers needs the event table, which the public source tree does not include
    if rules::event_table_present() {
        assert!(a.rules[0]
            .summary
            .contains("WHENEVER BURGLAR ALARM, ANY AREA"));
    } else {
        assert!(a.rules[0].summary.contains("Event "));
    }
    assert!(a.rules[2].summary.contains("Happy Birthday  Mr. Smith"));
}

#[test]
fn real_export_has_default_automation() {
    // A real ElkRP export of an account with default automation items. TESTING is git-ignored; ELKEDITOR_TEST_EXPORT can name another file.
    let p = std::env::var_os("ELKEDITOR_TEST_EXPORT")
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join("TESTING/default-automation-export.txt"));
    if !p.exists() {
        return;
    }
    let (_, m) = open(&read(&p));
    let a = m.automation.unwrap();
    assert_eq!(a.rule_packets_used, 0);
    assert!(a.texts.is_empty() && a.rules.is_empty());
    assert_eq!(
        a.outputs
            .iter()
            .filter(|o| o.name.starts_with("Output "))
            .count(),
        64
    );
    assert_eq!(a.tasks[0].name, "Task 01");
    assert!(a.tasks[0].show);
    assert_eq!(a.customs[0].value, 600);
    assert_eq!(a.customs[1].value, 65535);
    assert_eq!(a.customs[1].name, "FireDuration");
    assert_eq!(a.lights[0].name, "Light01");
    assert_eq!(a.voice_messages.len(), 115);
}

#[test]
fn edits_of_names_flags_and_voice_words_round_trip_and_touch_only_that_record() {
    let text = sample();
    let (mut f, mut m) = open(&text);
    {
        let a = auto(&mut m);
        a.tasks[0].name = "Good Night".into();
        a.tasks[0].show = true;
        a.tasks[0].voice[0] = 471;
        a.outputs[2].name = "Garage Door".into();
        a.outputs[2].show = true;
        a.outputs[2].voice = vec![22, 23, 0, 0, 0, 65535];
        a.counters[4] = Some("Visitors".into());
        a.thermostats[1] = Some("Upstairs".into());
        a.customs[12].value = 90;
        a.customs[12].name = "PoolPumpMin".into();
        a.customs[13].type_ = 2;
        a.customs[13].value = (22 << 8) | 30;
        a.customs[0].value = 300;
        a.customs[0].name = "ignored".into(); // durations keep their fixed name
        a.lights[0].name = "Porch".into();
        a.lights[0].type_ = 2;
        a.lights[0].format = 2;
        a.lights[0].two_way = true;
        a.lights[0].show = false;
        a.lights[200].name = "Pool Light".into();
        a.lights[200].show = true;
        a.voice_messages[4].words = vec![1, 2, 3, 4, 5, 6];
    }
    m.apply_to(&mut f).unwrap();
    let r = reload(&f);
    assert_eq!(r.tasks[0].name, "Good Night");
    assert!(r.tasks[0].show);
    assert_eq!(r.tasks[0].voice[0], 471);
    assert_eq!(r.outputs[2].name, "Garage Door");
    assert!(r.outputs[2].show);
    assert_eq!(r.outputs[2].voice, vec![22, 23, 0, 0, 0, 65535]);
    assert_eq!(r.counters[4].as_deref(), Some("Visitors"));
    assert_eq!(r.thermostats[1].as_deref(), Some("Upstairs"));
    assert_eq!(r.customs[12].value, 90);
    assert_eq!(r.customs[12].name, "PoolPumpMin");
    assert_eq!(r.customs[13].type_, 2);
    assert_eq!(r.customs[13].value, (22 << 8) | 30);
    assert_eq!(r.customs[0].value, 300);
    assert_eq!(r.customs[0].name, "BurgDuration");
    assert_eq!(r.lights[0].name, "Porch");
    assert_eq!((r.lights[0].type_, r.lights[0].format), (2, 2));
    assert!(r.lights[0].two_way && !r.lights[0].show);
    assert_eq!(r.lights[200].name, "Pool Light");
    assert_eq!(r.lights[200].id, "M9");
    assert!(r.lights[200].show);
    assert_eq!(r.voice_messages[4].words, vec![1, 2, 3, 4, 5, 6]);
    // nothing else moved
    assert_eq!(r.tasks[1].name, "Go to Work");
    assert_eq!(r.lights[1].name, "Foyer Lights");
    assert_eq!(r.texts.len(), 1);
    assert_eq!(r.rules.len(), 6);
    // the file differs from the original only in the lines of the edited records
    let before: Vec<&str> = text.split("\r\n").collect();
    let after_text = f.to_text();
    let after: Vec<&str> = after_text.split("\r\n").collect();
    assert_eq!(before.len(), after.len());
    let diff = (0..before.len()).filter(|&i| before[i] != after[i]).count();
    assert!((1..=400).contains(&diff), "{diff}");
}

#[test]
fn invalid_values_are_refused() {
    let cases: Vec<Box<dyn Fn(&mut AutomationDto)>> = vec![
        Box::new(|a| a.tasks[0].name = "this name is far too long".into()),
        Box::new(|a| a.outputs[0].voice[1] = 70000),
        Box::new(|a| a.customs[14].type_ = 3),
        Box::new(|a| {
            a.customs[14].type_ = 2;
            a.customs[14].value = 25 << 8
        }),
        Box::new(|a| a.lights[0].name = "1234567890123456".into()),
        Box::new(|a| a.counters[0] = Some("bad\u{c3}\u{a9}".into())),
    ];
    for c in cases {
        let (mut f, mut m) = open(&sample());
        c(auto(&mut m));
        assert!(is_format_err(m.apply_to(&mut f)));
    }
}

#[test]
fn text_strings_can_be_edited_added_and_pointers_follow() {
    let (mut f, mut m) = open(&sample());
    {
        let a = auto(&mut m);
        a.texts[0].text = "Happy Birthday to Mr. Smith, the best customer ever".into();
        a.texts.push(TextDto {
            index: 0,
            text: "Second".into(),
            ..Default::default()
        });
    }
    m.apply_to(&mut f).unwrap();
    let r = reload(&f);
    assert_eq!(r.texts.len(), 2);
    assert_eq!(
        r.texts[0].text,
        "Happy Birthday to Mr. Smith, the best customer ever"
    );
    assert_eq!(r.texts[1].text, "Second");
    assert_eq!(
        r.text_packets_used,
        r.texts.iter().map(|t| t.packets).sum::<i32>()
    );
    assert!(r.rules[2]
        .summary
        .contains("Happy Birthday to Mr. Smith, the best customer ever"));
    assert_eq!((r.texts[0].used_by_rules, r.texts[1].used_by_rules), (1, 0));
    assert_eq!(r.rule_packets_used, 12);
}

#[test]
fn a_text_string_that_a_rule_uses_cannot_be_deleted_but_an_unused_one_can() {
    let (mut f, mut m) = open(&sample());
    auto(&mut m).texts.clear();
    assert!(is_format_err(m.apply_to(&mut f)));
    let (mut f, mut m) = open(&sample());
    auto(&mut m).texts.push(TextDto {
        index: 0,
        text: "Extra".into(),
        ..Default::default()
    });
    m.apply_to(&mut f).unwrap();
    let (mut f2, mut m2) = open(&f.to_text());
    auto(&mut m2).texts.remove(1);
    m2.apply_to(&mut f2).unwrap();
    let r = reload(&f2);
    assert_eq!(r.texts.len(), 1);
    assert_eq!(r.text_packets_used, 4);
    assert!(r.rules[2].summary.contains("Happy Birthday  Mr. Smith"));
}

#[test]
fn text_with_escape_round_trips_and_too_long_text_is_refused() {
    let (mut f, mut m) = open(&sample());
    auto(&mut m).texts[0].text = "Count {ff:01:00:05} now".into();
    m.apply_to(&mut f).unwrap();
    assert_eq!(reload(&f).texts[0].text, "Count {ff:01:00:05} now");
    let (mut f, mut m) = open(&sample());
    auto(&mut m).texts[0].text = "x".repeat(80);
    assert!(is_format_err(m.apply_to(&mut f)));
    let (mut f, mut m) = open(&sample());
    auto(&mut m).texts[0].text = "tab\there".into();
    assert!(is_format_err(m.apply_to(&mut f)));
}

#[test]
fn rule_comments_delete_and_reorder() {
    let (mut f, mut m) = open(&sample());
    auto(&mut m).rules[0].comment = "Burglar alarm lights".into();
    auto(&mut m).rules[4].comment = "Armed away".into();
    m.apply_to(&mut f).unwrap();
    let r = reload(&f);
    assert_eq!(r.rules[0].comment, "Burglar alarm lights");
    assert_eq!(r.rules[4].comment, "Armed away");
    assert_eq!(r.rule_packets_used, 12);
    // delete rule 2 (one packet) and move the comment with its rule
    let (mut f2, mut m2) = open(&f.to_text());
    auto(&mut m2).rules.remove(1);
    m2.apply_to(&mut f2).unwrap();
    let r2 = reload(&f2);
    assert_eq!(r2.rules.len(), 5);
    assert_eq!(r2.rule_packets_used, 11);
    assert_eq!(r2.rules[3].comment, "Armed away");
    assert!(r2.rules[1].summary.contains("Happy Birthday  Mr. Smith"));
    assert_eq!(r2.rules[4].comment, "");
    // swapping two rules keeps the group structure
    let (mut f3, mut m3) = open(&sample());
    auto(&mut m3).rules.swap(0, 3);
    m3.apply_to(&mut f3).unwrap();
    let r3 = reload(&f3);
    if rules::event_table_present() {
        assert!(r3.rules[0].summary.contains("ENTRY DELAY"));
        assert!(r3.rules[3].summary.contains("BURGLAR ALARM"));
    } else {
        assert_ne!(r3.rules[0].summary, r3.rules[3].summary);
    }
    assert_eq!(r3.rules.len(), 6);
    assert_eq!(r3.rule_packets_used, 12);
}

#[test]
fn rule_errors_are_refused() {
    let (mut f, mut m) = open(&sample());
    auto(&mut m).rules[0].packets[0] = vec![0; 8];
    assert!(is_format_err(m.apply_to(&mut f)));
    let (mut f, mut m) = open(&sample());
    auto(&mut m).rules[0].comment = "two\r\nlines".into();
    assert!(is_format_err(m.apply_to(&mut f)));
    let (mut f, mut m) = open(&sample());
    for i in 0..520 {
        auto(&mut m).rules.push(RuleDto {
            n: 100 + i,
            packets: vec![vec![89, 1, 3, 235, 26, 3, 0, 0]],
            ..Default::default()
        });
    }
    assert!(is_format_err(m.apply_to(&mut f)));
}

#[test]
fn model_json_round_trip_keeps_automation() {
    let text = sample();
    let mut f = AccountFile::parse(&text).unwrap();
    let m = AccountModel::from(&f).unwrap();
    let back = AccountModel::from_json(&m.to_json()).unwrap();
    back.apply_to(&mut f).unwrap();
    assert_eq!(f.to_text(), text);
}

#[test]
fn vocabulary_and_event_tables_are_embedded() {
    assert_eq!(AutomationMeta::words().get(21), Some("Zero"));
    assert!(AutomationMeta::words().len() > 400);
    assert_eq!(AutomationMeta::voice_message_names().len(), 115);
    assert!(AutomationMeta::voice_message_names()
        .get(1)
        .unwrap()
        .contains("Keypad Panic Alarm"));
    if rules::event_table_present() {
        assert_eq!(
            rules::event_names().get(1003),
            Some("BURGLAR ALARM, ANY AREA")
        );
        assert!(rules::event_names().len() > 1000);
    } else {
        assert_eq!(rules::event_names().len(), 0);
    }
    assert_eq!(AutomationMeta::output_names().len(), 205);
}

#[test]
fn old_software_without_the_block_is_left_alone() {
    // a 4.x export has a different layout; the lines after the report codes are copied through unchanged
    let text = sample().replace("\"5.0.12\"", "\"4.3.6\"");
    let f = AccountFile::parse(&text).unwrap();
    assert!(f.automation.is_none());
    assert!(AccountModel::from(&f).unwrap().automation.is_none());
    assert_eq!(f.to_text(), text);
}

#[test]
fn a_model_with_automation_is_refused_for_a_file_without_the_block() {
    let (_, m) = open(&sample());
    let mut f = AccountFile::parse(&sample()).unwrap();
    f.automation = None;
    let e = m.apply_to(&mut f).unwrap_err();
    assert_eq!(
        e.message(),
        "this file has no automation section (outputs, tasks, rules ...)"
    );
}
