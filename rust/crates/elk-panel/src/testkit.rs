//! Helpers that fill an `elk_core::simulated_panel::SimulatedPanel` the way the C# `PanelSyncTests` do. Public so the tests, the golden example and the
//! app's own tests share one definition of each scenario.

use elk_account::AccountFile;
use elk_core::simulated_panel::SimulatedPanel;
use elk_core::DefaultRecords;

/// `Put`: stores a record under `(section, item)`.
pub fn put(p: &SimulatedPanel, section: u8, item: i32, data: &[u8]) {
    p.set_record(section, item as u8, data.to_vec());
}

/// `Seed`: fills the panel with every record the sync code may read: zeros by default, then the file's own records.
pub fn seed(p: &SimulatedPanel, f: &AccountFile) {
    for n in 1..=199 {
        put(p, 1, n, &[0; 28]);
        put(p, 10, n, &[0; 4]);
    }
    for n in 1..=8 {
        put(p, 3, n, &[0; 24]);
        put(p, 8, n, &[0; 88]);
        put(p, 9, n, &[0; 20]);
    }
    for n in 1..=16 {
        put(p, 4, n, &[0; 148]);
    }
    for n in 1..=208 {
        put(p, 5, n, &[0; 32]);
        put(p, 12, n, &[0; 4]);
    }
    if let Some(fw) = f.wireless().ok().flatten() {
        for (i, w) in fw.iter().enumerate() {
            put(p, 15, i as i32 + 1, w);
        }
    }
    put(p, 7, 1, &f.globals().ok().flatten().unwrap_or_else(|| vec![0; 48]));
    put(p, 7, 2, &[0; 200]); // dawn / dusk
    put(p, 11, 1, &[0; 24]);
    put(p, 16, 1, &[0; 96]);
    for e in f.users.entries.iter().filter(|e| e.number <= 199) {
        put(p, 1, e.number, &e.bytes);
    }
    for e in &f.areas.entries {
        put(p, 3, e.number, &e.bytes);
    }
    for e in &f.keypads.entries {
        put(p, 4, e.number, &e.bytes);
    }
    for e in &f.zones.entries {
        put(p, 5, e.number, &e.bytes);
    }
    for e in &f.phones.entries {
        put(p, 8, e.number, &e.bytes);
    }
    for e in &f.area_rcs.entries {
        put(p, 9, e.number, &e.bytes);
    }
    for e in f.user_rcs.entries.iter().filter(|e| e.number <= 199) {
        put(p, 10, e.number, &e.bytes);
    }
    put(p, 11, 1, &f.system_rc);
    for e in &f.zone_rcs.entries {
        put(p, 12, e.number, &e.bytes);
    }
    put(p, 16, 1, &f.keypad_rc);
}

/// Fills the simulated panel's automation items (all kinds) and its rule / text packets from the file's automation block (nothing when it has none).
/// Rust only (see `auto_sync`).
pub fn seed_automation(p: &SimulatedPanel, f: &AccountFile) {
    use elk_core::AutoKind;
    let Some(a) = f.automation.as_ref() else {
        return;
    };
    for k in AutoKind::ALL {
        for n in 1..=k.count() {
            let (sec, part, item) = k.locate(n);
            let data = match k {
                AutoKind::Task => &a.tasks[n - 1],
                AutoKind::Counter => &a.counters[n - 1],
                AutoKind::Thermostat => &a.counters[63 + n],
                AutoKind::AlarmDuration => &a.customs[n - 1],
                AutoKind::CustomSetting => &a.customs[11 + n],
                AutoKind::Output => &a.outputs[n - 1],
                AutoKind::Light => &a.lights[n - 1],
                AutoKind::Voice => &a.voice[n - 1],
            };
            p.set_auto_item(sec, part, item, data.clone());
        }
    }
    p.set_packets(0, &a.packets);
}

/// `SeedDefaults`: a panel in factory-default state.
pub fn seed_defaults(p: &SimulatedPanel) {
    for n in 1..=199 {
        put(p, 1, n, &DefaultRecords::user(n));
        put(p, 10, n, &DefaultRecords::user_rc());
    }
    for n in 1..=8 {
        put(p, 3, n, &DefaultRecords::area(n));
        put(p, 8, n, &DefaultRecords::phone());
        put(p, 9, n, &DefaultRecords::area_rc());
    }
    for n in 1..=16 {
        put(p, 4, n, &DefaultRecords::keypad());
    }
    for n in 1..=208 {
        put(p, 5, n, &DefaultRecords::zone(n));
        put(p, 12, n, &DefaultRecords::zone_rc());
    }
    put(p, 7, 1, &[0; 48]);
    put(p, 7, 2, &[0; 200]);
    put(p, 11, 1, &[0; 24]);
    put(p, 16, 1, &[0; 96]);
    for i in 1..=3 {
        put(p, 15, i, &[0; 260]);
    }
}

/// The leftovers of `ResetMissing_Only_Touches_...`: a custom zone 150, a user 50 with a code, a phone and area 3 that the template file does not have.
pub fn add_reset_leftovers(p: &SimulatedPanel) {
    let mut z = elk_core::ZoneRecord::new();
    z.set_name("Old Zone");
    z.set_function(1);
    put(p, 5, 150, &z.bytes);
    let mut u = DefaultRecords::user(50);
    u[0] = 1;
    u[1] = 2;
    put(p, 1, 50, &u);
    let mut ph = DefaultRecords::phone();
    ph[0] = b'5';
    put(p, 8, 2, &ph);
    let mut a = DefaultRecords::area(3);
    a[0] = b'X';
    put(p, 3, 3, &a);
}

/// The extras of `Pull_Overwrites_File_Records_And_Adds_Only_Used_Ones`: a renamed zone 3, a used zone 150, an unused default-looking zone 151,
/// a user 50 with a code and a user 51 without one. Needs the panel seeded from the same file first.
pub fn add_pull_extras(p: &SimulatedPanel) {
    let mut z3 = p.record(5u8, 3).unwrap_or_else(|| vec![0; 32]);
    for b in &mut z3[4..20] {
        *b = 0;
    }
    z3[4..14].copy_from_slice(b"Panel Name");
    put(p, 5, 3, &z3);
    let mut z_new = [0u8; 32];
    z_new[0] = 1;
    z_new[4..12].copy_from_slice(b"New Zone");
    put(p, 5, 150, &z_new);
    let mut z_default = [0u8; 32];
    z_default[4..12].copy_from_slice(b"Zone 151");
    put(p, 5, 151, &z_default);
    let mut u = [0u8; 28];
    u[0] = 1;
    u[1] = 2;
    u[6] = 0xE4;
    u[9..15].copy_from_slice(b"PULLED");
    put(p, 1, 50, &u);
    put(p, 1, 51, &[0; 28]);
}

/// Sets a named scenario up (the names the golden manifest uses): `seed`, `defaults` (defaults plus the reset leftovers) or `pull` (seed plus the pull extras).
pub fn setup(name: &str, p: &SimulatedPanel, f: &AccountFile) -> Result<(), String> {
    match name {
        "seed" => {
            seed(p, f);
            seed_automation(p, f);
        }
        "defaults" => {
            seed_defaults(p);
            add_reset_leftovers(p);
        }
        "pull" => {
            seed(p, f);
            seed_automation(p, f);
            add_pull_extras(p);
        }
        other => return Err(format!("unknown panel setup '{other}'")),
    }
    Ok(())
}
