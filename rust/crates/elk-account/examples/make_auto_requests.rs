//! Writes the automation request bodies for the golden tests (`tests/fixtures/requests/s<i>_auto_*.json`): edits of every item
//! type plus error cases. They are POSTed to the C# server (rust/original-automation-build) by tests/fixtures/regen.ps1.
//! Usage: cargo run --example make_auto_requests -- <sample dir> <out dir>

#![allow(clippy::type_complexity)]

use elk_account::*;
use std::fs;

const SAMPLES: [&str; 4] = [
    "ElkRPAcctSample Export",
    "Sample Communicator",
    "Sample Zones Import",
    "Sample Zones Import Export",
];

fn auto(m: &mut AccountModel) -> &mut AutomationDto {
    m.automation.as_mut().expect("automation")
}

fn edit_outputs(m: &mut AccountModel) {
    let a = auto(m);
    a.outputs[0].name = "Front Gate".into();
    a.outputs[0].show = true;
    a.outputs[2].name = "Garage Door".into();
    a.outputs[2].show = false;
    a.outputs[2].voice = vec![22, 23, 0, 0, 0, 65535];
    a.outputs[63].name = "Last output 64".into();
}

fn edit_tasks(m: &mut AccountModel) {
    let a = auto(m);
    a.tasks[0].name = "Good Night".into();
    a.tasks[0].show = true;
    a.tasks[0].voice[0] = 471;
    a.tasks[1].show = false;
    a.tasks[31].name = "Padded   ".into();
    a.tasks[31].voice = vec![1, 2, 3];
}

fn edit_counters(m: &mut AccountModel) {
    let a = auto(m);
    a.counters[4] = Some("Visitors".into());
    a.counters[63] = Some("Counter 64".into());
    a.counters[10] = None;
    a.thermostats[1] = Some("Upstairs".into());
    a.thermostats[15] = Some("".into());
}

fn edit_customs(m: &mut AccountModel) {
    let a = auto(m);
    a.customs[12].value = 90;
    a.customs[12].name = "PoolPumpMin".into();
    a.customs[13].type_ = 2;
    a.customs[13].value = (22 << 8) | 30;
    a.customs[14].type_ = 1;
    a.customs[14].value = 0xFFFFFF;
    a.customs[0].value = 300;
    a.customs[0].name = "ignored".into();
    a.customs[1].value = 65535;
}

fn edit_lights(m: &mut AccountModel) {
    let a = auto(m);
    a.lights[0].name = "Porch".into();
    a.lights[0].type_ = 2;
    a.lights[0].format = 2;
    a.lights[0].two_way = true;
    a.lights[0].show = false;
    a.lights[200].name = "Pool Light".into();
    a.lights[200].show = true;
    a.lights[255].voice = vec![9, 8, 7, 6, 5, 4];
    a.lights[100].type_ = 7;
    a.lights[100].format = 4;
}

fn edit_voice(m: &mut AccountModel) {
    let a = auto(m);
    a.voice_messages[4].words = vec![1, 2, 3, 4, 5, 6];
    a.voice_messages[114].words = vec![65535, 0, 65535, 0, 1, 1];
}

fn edit_texts(m: &mut AccountModel) {
    let a = auto(m);
    if let Some(t) = a.texts.first_mut() {
        t.text = "Happy Birthday to Mr. Smith, the best customer ever".into();
    }
    a.texts.push(TextDto {
        text: "Second".into(),
        ..Default::default()
    });
    a.texts.push(TextDto {
        text: "Count {ff:01:00:05} now".into(),
        ..Default::default()
    });
    a.texts.push(TextDto {
        text: "{ff:01:00:05}".into(),
        ..Default::default()
    });
}

fn edit_texts_delete_new(m: &mut AccountModel) {
    let a = auto(m);
    a.texts.push(TextDto {
        text: "Extra".into(),
        ..Default::default()
    });
}

fn edit_rules_comment(m: &mut AccountModel) {
    let a = auto(m);
    if let Some(r) = a.rules.get_mut(0) {
        r.comment = "Burglar alarm lights".into();
    }
    if let Some(r) = a.rules.get_mut(4) {
        r.comment = "Armed away".into();
    }
}

fn edit_rules_delete(m: &mut AccountModel) {
    let a = auto(m);
    edit_rules_comment_inner(a);
    if a.rules.len() > 1 {
        a.rules.remove(1);
    }
}

fn edit_rules_comment_inner(a: &mut AutomationDto) {
    if let Some(r) = a.rules.get_mut(0) {
        r.comment = "first".into();
    }
    if let Some(r) = a.rules.get_mut(4) {
        r.comment = "Armed away".into();
    }
}

fn edit_rules_reorder(m: &mut AccountModel) {
    let a = auto(m);
    edit_rules_comment_inner(a);
    if a.rules.len() > 3 {
        a.rules.swap(0, 3);
    }
}

fn edit_rules_packets(m: &mut AccountModel) {
    let a = auto(m);
    a.rules.push(RuleDto {
        n: 100,
        comment: "added".into(),
        packets: vec![vec![89, 1, 3, 235, 26, 3, 0, 0]],
        summary: String::new(),
        ..Default::default()
    });
    if let Some(r) = a.rules.get_mut(1) {
        r.packets[0][1] = 7;
    }
}

fn edit_all(m: &mut AccountModel) {
    edit_outputs(m);
    edit_tasks(m);
    edit_counters(m);
    edit_customs(m);
    edit_lights(m);
    edit_voice(m);
    edit_texts(m);
    edit_rules_delete(m);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (dir, out) = (&args[1], &args[2]);
    fs::create_dir_all(out).unwrap();
    let mut sample0: Option<AutomationDto> = None;
    for (i, name) in SAMPLES.iter().enumerate() {
        let text = decode_latin1(&fs::read(format!("{dir}/{name}")).unwrap());
        let f = AccountFile::parse(&text).unwrap();
        let base = AccountModel::from(&f).unwrap();
        if i == 0 {
            sample0 = base.automation.clone();
        }
        let w = |tag: &str, m: &AccountModel| {
            fs::write(format!("{out}/s{i}_auto_{tag}.json"), m.to_json()).unwrap()
        };
        if base.automation.is_none() {
            // a file without the block: the model of sample 0 is refused
            let mut m = base.clone();
            m.automation = sample0.clone();
            w("foreign", &m);
            continue;
        }
        w("plain", &base);
        let edits: [(&str, fn(&mut AccountModel)); 12] = [
            ("outputs", edit_outputs),
            ("tasks", edit_tasks),
            ("counters", edit_counters),
            ("customs", edit_customs),
            ("lights", edit_lights),
            ("voice", edit_voice),
            ("texts", edit_texts),
            ("texts_add", edit_texts_delete_new),
            ("rules_comment", edit_rules_comment),
            ("rules_delete", edit_rules_delete),
            ("rules_reorder", edit_rules_reorder),
            ("rules_packets", edit_rules_packets),
        ];
        for (tag, f) in edits {
            let mut m = base.clone();
            f(&mut m);
            w(tag, &m);
        }
        let mut m = base.clone();
        edit_all(&mut m);
        w("all", &m);
        // one more: everything plus an ordinary zone edit, as the page sends it
        let mut m = base.clone();
        edit_all(&mut m);
        m.zones[0].name = "Edited Zone".into();
        w("all_zone", &m);

        let bad = |tag: &str, f: &dyn Fn(&mut AutomationDto)| {
            let mut m = base.clone();
            f(auto(&mut m));
            w(&format!("bad_{tag}"), &m);
        };
        bad("taskname", &|a| {
            a.tasks[0].name = "this name is far too long".into()
        });
        bad("taskchar", &|a| a.tasks[0].name = "caf\u{e9}".into());
        bad("tasknum", &|a| a.tasks[0].n = 33);
        bad("outputnum", &|a| a.outputs[0].n = 0);
        bad("voiceword", &|a| a.outputs[0].voice[1] = 70000);
        bad("voiceneg", &|a| a.tasks[3].voice[0] = -1);
        bad("customtype", &|a| a.customs[14].type_ = 3);
        bad("customnum", &|a| a.customs[14].n = 40);
        bad("customvalue", &|a| a.customs[14].value = 0x1000000);
        bad("customtime", &|a| {
            a.customs[14].type_ = 2;
            a.customs[14].value = 25 << 8
        });
        bad("customminute", &|a| {
            a.customs[14].type_ = 2;
            a.customs[14].value = 60
        });
        bad("lightname", &|a| {
            a.lights[0].name = "1234567890123456".into()
        });
        bad("lighttype", &|a| a.lights[0].type_ = 8);
        bad("lightformat", &|a| a.lights[0].format = -1);
        bad("lightnum", &|a| a.lights[0].n = 257);
        bad("voicenum", &|a| a.voice_messages[0].n = 116);
        bad("counter", &|a| a.counters[0] = Some("bad\u{e9}".into()));
        bad("counterlong", &|a| {
            a.thermostats[0] = Some("1234567890123".into())
        });
        bad("textsclear", &|a| a.texts.clear());
        bad("textesc", &|a| {
            a.texts.push(TextDto {
                text: "tab\there".into(),
                ..Default::default()
            })
        });
        bad("textlong", &|a| {
            a.texts.push(TextDto {
                text: "x".repeat(80),
                ..Default::default()
            })
        });
        bad("textlong79", &|a| {
            a.texts.push(TextDto {
                text: "x".repeat(79),
                ..Default::default()
            })
        });
        bad("textindex", &|a| {
            a.texts.push(TextDto {
                index: 9,
                text: "x".into(),
                ..Default::default()
            })
        });
        bad("textdup", &|a| {
            if let Some(t) = a.texts.first().cloned() {
                a.texts.push(t);
                a.texts[0].text = "changed".into();
            } else {
                a.texts.push(TextDto {
                    index: 0,
                    text: "a".into(),
                    ..Default::default()
                });
                a.texts.push(TextDto {
                    index: 0,
                    text: "b".into(),
                    ..Default::default()
                });
            }
        });
        bad("textfull", &|a| {
            for _ in 0..70 {
                a.texts.push(TextDto {
                    text: "x".repeat(79),
                    ..Default::default()
                });
            }
        });
        bad("rulezero", &|a| {
            if let Some(r) = a.rules.get_mut(0) {
                r.packets[0] = vec![0; 8]
            } else {
                a.rules.push(RuleDto {
                    n: 1,
                    packets: vec![vec![0; 8]],
                    ..Default::default()
                })
            }
        });
        bad("rulecomment", &|a| {
            a.rules.push(RuleDto {
                n: 1,
                comment: "two\r\nlines".into(),
                packets: vec![vec![89, 1, 3, 235, 26, 3, 0, 0]],
                summary: String::new(),
                ..Default::default()
            })
        });
        bad("rulelong", &|a| {
            a.rules.push(RuleDto {
                n: 1,
                comment: "c".repeat(251),
                packets: vec![vec![89, 1, 3, 235, 26, 3, 0, 0]],
                summary: String::new(),
                ..Default::default()
            })
        });
        bad("rulepacket", &|a| {
            a.rules.push(RuleDto {
                n: 1,
                packets: vec![vec![89, 1, 3]],
                ..Default::default()
            })
        });
        bad("rulebyte", &|a| {
            a.rules.push(RuleDto {
                n: 1,
                packets: vec![vec![89, 1, 3, 256, 0, 0, 0, 0]],
                ..Default::default()
            })
        });
        bad("rulenone", &|a| {
            a.rules.push(RuleDto {
                n: 1,
                packets: vec![],
                ..Default::default()
            })
        });
        bad("rulefull", &|a| {
            for k in 0..520 {
                a.rules.push(RuleDto {
                    n: 100 + k,
                    packets: vec![vec![89, 1, 3, 235, 26, 3, 0, 0]],
                    ..Default::default()
                })
            }
        });
    }
}
