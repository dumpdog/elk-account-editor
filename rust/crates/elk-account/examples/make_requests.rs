//! Writes the edited-model request bodies used for the golden tests (`tests/fixtures/requests/*.json`).
//! Usage: cargo run --example make_requests -- <sample dir> <out dir>
//! The same files are POSTed to the C# server (see tests/fixtures/README) and its responses stored next to them.

use elk_account::*;
use std::fs;

fn flip(v: &mut Vec<String>, k: &str) {
    if let Some(i) = v.iter().position(|x| x == k) {
        v.remove(i);
    } else {
        v.push(k.to_string());
    }
}

fn edit_general(m: &mut AccountModel) {
    if let Some(z) = m.zones.first_mut() {
        z.name = "Edited Zone".into();
        z.definition = 3;
        z.type_ = 2;
        z.area = 2;
        flip(&mut z.flags, "chime");
        flip(&mut z.flags, "silent");
    }
    if m.zones.len() > 1 {
        m.zones[1].default_voice = true;
        m.zones[1].name = "Padded ".into();
    }
    for n in [100, 200] {
        if !m.zones.iter().any(|z| z.n == n) {
            let z = ZoneDto {
                n,
                name: format!("New {n}"),
                definition: 4,
                type_: 5,
                area: 3,
                flags: vec!["silent".into(), "chime".into()],
                default_voice: false,
            };
            let at = m
                .zones
                .iter()
                .position(|x| x.n > n)
                .unwrap_or(m.zones.len());
            m.zones.insert(at, z);
        }
    }
    let cl = m.code_length as usize;
    if m.users.len() > 1 {
        m.users[1].code = "1357924680".chars().take(cl).collect();
        m.users[1].name = "Caf\u{e9} \u{2603} Bob".into();
        flip(&mut m.users[1].flags, "master");
        flip(&mut m.users[1].flags, "area3");
    }
    if !m.users.iter().any(|u| u.n == 20) {
        m.users.push(UserDto {
            n: 20,
            name: "Twenty".into(),
            code: "2468024680".chars().take(cl).collect(),
            flags: vec!["arm".into(), "disarm".into()],
        });
    }
    if let Some(a) = m.areas.first_mut() {
        a.exit1 = 45;
        a.arm_window = 300;
        a.name = "Main Floor".into();
        flip(&mut a.flags, "night");
    }
    if let Some(k) = m.keypads.first_mut() {
        k.name = "Hall Keypad".into();
        k.f_keys[0].definition = 258;
        k.f_keys[0].name = "F One".into();
        k.area = 1;
    }
    if let Some(p) = m.phones.first_mut() {
        p.number = "8005551212".into();
        p.format = 6;
        p.attempts = 5;
        p.account_codes[1] = Some("009999".into());
        p.account_codes[2] = None;
        flip(&mut p.flags, "zoneBypass");
    } else if m.has_phones {
        m.phones.push(PhoneDto {
            n: 1,
            name: "Central".into(),
            number: "5551234".into(),
            format: 3,
            attempts: 2,
            account_codes: vec![Some("123456".into()); 8],
            flags: vec!["areaEvents".into()],
        });
    }
    if let Some(c) = m.comm.as_mut() {
        c.areas[0].codes[2] = 3;
        c.areas[0].abort_time = 20;
        c.system.codes[1] = 3;
        c.system.test_hours = 7;
        c.users[0].open = 2;
        c.zones[0].codes[3] = 9;
        c.keypads[0].codes[4] = 5;
    }
    if let Some(g) = m.globals.as_mut() {
        g[16] = 7;
        g[0] |= 8;
    }
    if let Some(w) = m.wireless.as_mut() {
        w[0][20] |= 1;
        w[0][166] = 0x35;
    }
    if m.zone_groups.len() > 3 {
        m.zone_groups[2] = 1;
    }
}

fn edit_prune(m: &mut AccountModel) {
    m.prune = true;
    m.zones.retain(|z| z.n != 5 && z.n != 6 && z.n != 7);
    if let Some(c) = m.comm.as_mut() {
        c.zones.retain(|z| z.n != 5 && z.n != 6);
        c.users.retain(|u| u.n != 3);
    }
    m.users.retain(|u| u.n != 3);
    m.phones.clear();
    if m.keypads.len() > 1 {
        m.keypads.pop();
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (dir, out) = (&args[1], &args[2]);
    fs::create_dir_all(out).unwrap();
    for (i, name) in [
        "ElkRPAcctSample Export",
        "Sample Communicator",
        "Sample Zones Import",
        "Sample Zones Import Export",
    ]
    .iter()
    .enumerate()
    {
        let text = decode_latin1(&fs::read(format!("{dir}/{name}")).unwrap());
        let f = AccountFile::parse(&text).unwrap();
        let base = AccountModel::from(&f).unwrap();
        let w = |tag: &str, m: &AccountModel| {
            fs::write(format!("{out}/s{i}_{tag}.json"), m.to_json()).unwrap()
        };
        w("plain", &base);
        let mut m = base.clone();
        edit_general(&mut m);
        w("edit", &m);
        let mut m = base.clone();
        edit_prune(&mut m);
        w("prune", &m);
        let mut m = base.clone();
        edit_general(&mut m);
        edit_prune(&mut m);
        w("editprune", &m);
        // error cases
        let bad = |tag: &str, f: &dyn Fn(&mut AccountModel)| {
            let mut m = base.clone();
            f(&mut m);
            w(tag, &m);
        };
        bad("bad_zonename", &|m| {
            m.zones[0].name = "12345678901234567".into()
        });
        bad("bad_zonearea", &|m| m.zones[0].area = 9);
        bad("bad_zonetype", &|m| m.zones[0].type_ = 8);
        bad("bad_code", &|m| m.users[0].code = "12".into());
        bad("bad_group", &|m| {
            if !m.zone_groups.is_empty() {
                m.zone_groups[0] = 1
            }
        });
        bad("bad_group160", &|m| {
            if m.zone_groups.len() > 10 {
                m.zone_groups[10] = 1
            }
        });
        bad("bad_rc", &|m| {
            if let Some(c) = m.comm.as_mut() {
                c.system.codes[0] = 256
            }
        });
        bad("bad_format", &|m| {
            if let Some(p) = m.phones.first_mut() {
                p.format = 8
            }
        });
        bad("bad_zone209", &|m| {
            m.zones.push(ZoneDto {
                n: 209,
                name: "x".into(),
                ..Default::default()
            })
        });
        bad("bad_noarea", &|m| {
            m.prune = true;
            m.areas.clear()
        });
        bad("bad_noarea_keep", &|m| {
            m.prune = true;
            m.areas.retain(|a| a.n != 1);
            if m.areas.is_empty() {
                m.areas.push(AreaDto {
                    n: 2,
                    name: "Two".into(),
                    ..Default::default()
                })
            }
        });
        bad("bad_globals", &|m| {
            if let Some(g) = m.globals.as_mut() {
                g[3] = 300
            }
        });
    }
}
