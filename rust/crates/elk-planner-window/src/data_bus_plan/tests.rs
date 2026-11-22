//! Ports of `port/tests/Elk.Tests/DataBusPlanTests.cs`, plus JSON and number-format checks.

use super::*;

fn d(model: &str, addr: i32) -> BusDevice {
    BusDevice {
        model: model.into(),
        address: addr,
        ..Default::default()
    }
}
fn dn(model: &str, addr: i32, name: &str) -> BusDevice {
    BusDevice {
        name: name.into(),
        ..d(model, addr)
    }
}
fn dma(model: &str, addr: i32, ma: f64) -> BusDevice {
    BusDevice {
        ma,
        ..d(model, addr)
    }
}
fn r(name: &str, ft: f64, devices: Vec<BusDevice>) -> BusRun {
    BusRun {
        name: name.into(),
        length_ft: ft,
        devices,
    }
}
fn direct(ft: f64, devices: Vec<BusDevice>) -> MainRun {
    MainRun {
        kind: "direct".into(),
        name: "Bus".into(),
        run: r("Bus", ft, devices),
        ..Default::default()
    }
}
fn dbh(feed: f64, hubs: Vec<BusHub>) -> MainRun {
    MainRun {
        kind: "dbh".into(),
        feed_ft: feed,
        hubs,
        ..Default::default()
    }
}
fn hub(crossover: f64, runs: Vec<BusRun>) -> BusHub {
    BusHub {
        crossover_ft: crossover,
        home_runs: runs,
        ..Default::default()
    }
}
fn dbhr(feed: f64, branches: Vec<Vec<BusRun>>) -> MainRun {
    let mut m = MainRun {
        kind: "dbhr".into(),
        feed_ft: feed,
        ..Default::default()
    };
    for i in 0..4 {
        m.branches.push(BusBranch {
            runs: branches.get(i).cloned().unwrap_or_default(),
        });
    }
    m
}
fn p(runs: Vec<MainRun>) -> BusPlan {
    BusPlan {
        runs,
        ..Default::default()
    }
}
fn has(res: &PlanResult, level: &str, text: &str) -> bool {
    res.issues
        .iter()
        .any(|i| i.level == level && i.text.to_lowercase().contains(&text.to_lowercase()))
}
fn items(res: &PlanResult) -> String {
    res.checklist
        .iter()
        .flat_map(|g| g.items.iter().cloned())
        .collect::<Vec<_>>()
        .join("\n")
}
fn keypads(n: i32) -> Vec<BusRun> {
    (1..=n)
        .map(|i| r(&format!("R{}", i), 10.0, vec![d("Keypad", i)]))
        .collect()
}

#[test]
fn a_simple_single_run_is_clean_and_needs_jp3() {
    let res = check(&p(vec![direct(
        150.0,
        vec![dn("Keypad", 1, "Hall"), dn("M1XIN", 3, "Garage")],
    )]));
    assert_eq!(res.errors(), 0);
    assert_eq!(res.warnings(), 0);
    assert!(res.jp3);
    assert_eq!(res.main_bus_ft, 150.0);
    assert_eq!(res.devices, 2);
    let c = items(&res);
    assert!(c.contains("JP3 (terminating jumper on the M1 control): INSTALL"));
    assert!(c.contains("Terminate the last device only: M1XIN \"Garage\" (address 3)"));
}

#[test]
fn two_runs_remove_jp3_and_more_than_two_is_an_error() {
    let two = check(&p(vec![
        direct(100.0, vec![d("Keypad", 1)]),
        direct(100.0, vec![d("Keypad", 2)]),
    ]));
    assert!(!two.jp3);
    assert!(items(&two).contains("REMOVE it"));
    let three = check(&p(vec![
        direct(10.0, vec![d("Keypad", 1)]),
        direct(10.0, vec![d("Keypad", 2)]),
        direct(10.0, vec![d("Keypad", 3)]),
    ]));
    assert!(has(&three, "error", "No more than 2 runs"));
}

#[test]
fn addresses_may_repeat_across_types_but_not_within_a_type() {
    assert_eq!(
        check(&p(vec![direct(
            50.0,
            vec![d("Keypad", 3), d("M1XIN", 3), d("M1XOVR", 3), d("M1XSP", 3)]
        )]))
        .errors(),
        0
    );
    assert!(has(
        &check(&p(vec![direct(50.0, vec![d("Keypad", 4), d("M1KAM", 4)])])),
        "error",
        "Type 1"
    ));
    assert!(has(
        &check(&p(vec![direct(50.0, vec![d("M1XIN", 5), d("M1XRFEG", 5)])])),
        "error",
        "Type 2"
    ));
    assert!(has(
        &check(&p(vec![direct(50.0, vec![d("Keypad", 2), d("Keypad", 2)])])),
        "error",
        "Address 2 is used by 2"
    ));
    let across = check(&p(vec![
        direct(50.0, vec![d("M1XIN", 6)]),
        direct(50.0, vec![d("M1XIN", 6)]),
    ]));
    assert!(has(&across, "error", "Address 6"));
}

#[test]
fn address_ranges() {
    let cases: [(&str, i32, bool); 12] = [
        ("Keypad", 0, true),
        ("Keypad", 16, false),
        ("Keypad", 17, true),
        ("M1XIN", 1, true),
        ("M1XIN", 2, false),
        ("M1XIN", 13, false),
        ("M1XIN", 14, true),
        ("M1XRFTWM", 6, true),
        ("M1XRFTWM", 5, false),
        ("M1XSP", 8, true),
        ("M1XOVR", 14, true),
        ("M1XSP", 7, false),
    ];
    for (model, address, error) in cases {
        let mut devs = vec![d(model, address)];
        if model == "M1XRFTWM" {
            devs.push(d("M1XRFTWM", 2));
        }
        let res = check(&p(vec![direct(50.0, devs)]));
        let got = res
            .issues
            .iter()
            .any(|i| i.level == "error" && i.text.contains("addresses run from"));
        assert_eq!(got, error, "{} at {}", model, address);
    }
}

#[test]
fn primary_transceiver_must_be_at_address_2_and_at_most_four() {
    assert!(has(
        &check(&p(vec![direct(50.0, vec![d("M1XRFTWM", 3)])])),
        "error",
        "primary M1XRFTWM transceiver must be at address 2"
    ));
    assert_eq!(
        check(&p(vec![direct(
            50.0,
            vec![
                d("M1XRFTWM", 2),
                d("M1XRFTWM", 3),
                d("M1XRFTWM", 4),
                d("M1XRFTWM", 5)
            ]
        )]))
        .errors(),
        0
    );
}

#[test]
fn hardwired_expanders_cost_wireless_groups_and_clash_with_wireless_zones() {
    let mut plan = p(vec![direct(
        50.0,
        vec![d("M1XRFTWM", 2), dn("M1XIN", 3, "A"), dn("M1XIN", 5, "B")],
    )]);
    let res = check(&plan);
    assert!(has(&res, "info", "at most 112 wireless zones remain"));
    plan.wireless_groups = vec![5];
    let clash = check(&plan);
    assert!(has(&clash, "error", "zones 65-80"));
    assert_eq!(clash.errors(), 1);
    assert_eq!(zones_at(2), (17, 32));
    assert_eq!(zones_at(13), (193, 208));
}

#[test]
fn direct_length_limit_and_missing_devices() {
    assert!(has(
        &check(&p(vec![
            direct(2500.0, vec![d("Keypad", 1)]),
            direct(1600.0, vec![d("Keypad", 2)])
        ])),
        "error",
        "4000 ft"
    ));
    assert!(has(
        &check(&p(vec![direct(3300.0, vec![d("Keypad", 1)])])),
        "warn",
        "close to"
    ));
    assert!(has(
        &check(&p(vec![direct(100.0, vec![])])),
        "error",
        "no devices"
    ));
}

#[test]
fn hub_home_runs_count_double_and_jacks_are_planned_in_order() {
    let h = hub(
        0.0,
        vec![
            r("Hall", 100.0, vec![d("Keypad", 1)]),
            r("Garage", 150.0, vec![d("M1XIN", 3)]),
            r("Up", 50.0, vec![d("Keypad", 2)]),
        ],
    );
    let res = check(&p(vec![dbh(2.0, vec![h])]));
    assert_eq!(res.main_bus_ft, 2.0 + 2.0 * (100.0 + 150.0 + 50.0));
    assert_eq!(res.errors(), 0);
    assert!(res.jp3);
    let c = items(&res);
    assert!(c.contains("Hub 1, J1: home run \"Hall\""));
    assert!(c.contains("Hub 1, J3: home run \"Up\""));
    assert!(c.contains("Hub 1, J4: insert the RJ45 terminating plug"));
    assert!(res.checklist.iter().any(|g| g.title.contains("T568A")));
    assert!(has(
        &check(&p(vec![dbh(
            2.0,
            vec![hub(0.0, vec![r("Far", 2000.0, vec![d("Keypad", 1)])])]
        )])),
        "error",
        "4000 ft"
    ));
    assert_eq!(
        check(&p(vec![dbh(
            1.0,
            vec![hub(0.0, vec![r("Far", 1999.0, vec![d("Keypad", 1)])])]
        )]))
        .errors(),
        0
    );
}

#[test]
fn last_hub_can_use_all_nine_jacks_and_ends_at_the_device_on_the_last_jack() {
    let eight = hub(0.0, keypads(8));
    let nine = hub(
        0.0,
        (1..=9)
            .map(|i| {
                r(
                    &format!("R{}", i),
                    10.0,
                    vec![dn("Keypad", i, &format!("K{}", i))],
                )
            })
            .collect(),
    );
    let r8 = check(&p(vec![dbh(1.0, vec![eight])]));
    assert_eq!(r8.errors(), 0);
    assert!(items(&r8).contains("Hub 1, J9: insert the RJ45 terminating plug"));
    let r9 = check(&p(vec![dbh(1.0, vec![nine])]));
    assert_eq!(r9.errors(), 0);
    assert!(has(
        &r9,
        "info",
        "Fit its terminating jumper instead of the plug"
    ));
    let c = items(&r9);
    assert!(c.contains("no terminating plug. Fit the terminating jumper on the device on the last jack (J9): Keypad \"K9\" (address 9)"));
    assert!(!c.contains("insert the RJ45 terminating plug"));
    assert!(has(
        &check(&p(vec![dbh(1.0, vec![hub(0.0, keypads(10))])])),
        "error",
        "nine jacks"
    ));
}

#[test]
fn terminating_at_the_last_device_can_be_chosen_even_when_a_jack_is_free() {
    let mut h = hub(
        0.0,
        vec![
            r("A", 20.0, vec![dn("Keypad", 1, "First")]),
            r("B", 20.0, vec![dn("M1XIN", 3, "Last")]),
        ],
    );
    h.terminate_at_device = true;
    let res = check(&p(vec![dbh(1.0, vec![h])]));
    assert_eq!(res.errors(), 0);
    let c = items(&res);
    assert!(c.contains("J2): M1XIN \"Last\" (address 3)"));
    assert!(!c.contains("insert the RJ45 terminating plug"));
    let mut empty = hub(0.0, vec![r("A", 20.0, vec![])]);
    empty.terminate_at_device = true;
    assert!(has(
        &check(&p(vec![dbh(1.0, vec![empty])])),
        "error",
        "no device"
    ));
    let full = hub(5.0, keypads(9));
    assert!(has(
        &check(&p(vec![dbh(
            1.0,
            vec![full, hub(0.0, vec![r("B", 10.0, vec![d("M1XIN", 3)])])]
        )])),
        "error",
        "crossover cable to the next hub"
    ));
}

#[test]
fn two_hubs_use_a_crossover_and_only_the_last_is_terminated() {
    let h1 = hub(
        10.0,
        (1..=3)
            .map(|i| r(&format!("A{}", i), 20.0, vec![d("Keypad", i)]))
            .collect(),
    );
    let h2 = hub(0.0, vec![r("B1", 20.0, vec![d("M1XIN", 3)])]);
    let res = check(&p(vec![dbh(1.0, vec![h1, h2])]));
    assert_eq!(res.errors(), 0);
    assert_eq!(res.main_bus_ft, 1.0 + 2.0 * (60.0 + 10.0 + 20.0));
    let c = items(&res);
    assert!(c.contains("Hub 1, J4: crossover cable"));
    assert!(c.contains("J1 of hub 2"));
    assert!(c.contains("Hub 2, J2: insert the RJ45 terminating plug"));
    assert!(!c.contains("Hub 1, J4: insert"));
}

#[test]
fn retrofit_hub_terminations_per_branch_and_jp1_jp3() {
    let one = check(&p(vec![dbhr(
        2.0,
        vec![
            vec![r("Hall", 80.0, vec![d("Keypad", 1)])],
            vec![
                r("A", 50.0, vec![d("Keypad", 2)]),
                r("B", 60.0, vec![d("M1XIN", 3)]),
            ],
        ],
    )]));
    assert_eq!(one.errors(), 0);
    assert!(one.jp3);
    let c = items(&one);
    assert!(c.contains("Branch 1: terminate the last device (Keypad (address 1)) AND fit JP2"));
    assert!(c.contains("Branch 2: terminate the last device of each home run and REMOVE JP3"));
    assert!(c.contains("Branch 3: not used"));
    assert!(c.contains("JP1 (terminates the input from the control): INSTALL"));
    let two = check(&p(vec![
        dbhr(1.0, vec![vec![r("A", 50.0, vec![d("Keypad", 1)])]]),
        dbhr(1.0, vec![vec![r("B", 50.0, vec![d("Keypad", 2)])]]),
    ]));
    assert!(!two.jp3);
    assert_eq!(two.errors(), 0);
}

#[test]
fn retrofit_hub_limits() {
    let long1 = check(&p(vec![dbhr(
        2.0,
        vec![vec![
            r("A", 2500.0, vec![d("Keypad", 1)]),
            r("B", 1600.0, vec![d("Keypad", 2)]),
        ]],
    )]));
    assert!(has(&long1, "error", "branch 1 is 4100 ft"));
    assert!(has(
        &check(&p(vec![dbhr(
            15.0,
            vec![vec![r("A", 50.0, vec![d("Keypad", 1)])]]
        )])),
        "error",
        "cannot be remoted"
    ));
    assert!(has(
        &check(&p(vec![dbhr(
            5.0,
            vec![vec![r("A", 50.0, vec![d("Keypad", 1)])]]
        )])),
        "warn",
        "short 4-conductor"
    ));
    let three = check(&p(vec![dbhr(
        2.0,
        vec![vec![
            r("A", 5.0, vec![d("Keypad", 1)]),
            r("B", 5.0, vec![d("Keypad", 2)]),
            r("C", 5.0, vec![d("Keypad", 3)]),
        ]],
    )]));
    assert!(has(&three, "error", "at most 2"));
    assert!(has(
        &check(&p(vec![dbhr(2.0, vec![vec![r("Empty", 5.0, vec![])]])])),
        "error",
        "no device to terminate"
    ));
    assert_eq!(
        check(&p(vec![dbhr(
            2.0,
            vec![vec![r("A", 900.0, vec![d("Keypad", 1)])]]
        )]))
        .main_bus_ft,
        2.0
    );
}

#[test]
fn power_is_checked_only_when_currents_are_entered() {
    assert!(has(
        &check(&p(vec![direct(50.0, vec![d("Keypad", 1)])])),
        "info",
        "No device currents"
    ));
    assert!(has(
        &check(&p(vec![direct(
            50.0,
            vec![dma("Keypad", 1, 700.0), dma("Keypad", 2, 700.0)]
        )])),
        "error",
        "1.25 A"
    ));
    assert!(has(
        &check(&p(vec![direct(50.0, vec![dma("Keypad", 1, 1100.0)])])),
        "warn",
        "near the 1.25 A"
    ));
    let res = check(&p(vec![dbhr(
        1.0,
        vec![
            vec![r("A", 5.0, vec![dma("Keypad", 1, 100.0)])],
            vec![r("B", 5.0, vec![dma("Keypad", 2, 100.0)])],
        ],
    )]));
    assert_eq!(res.current_ma, 100.0 + 100.0 + 30.0 + 40.0);
}

#[test]
fn empty_plan_asks_for_a_run() {
    let res = check(&p(vec![]));
    assert_eq!(res.errors(), 0);
    assert!(res.issues.iter().any(|i| i.level == "info"));
}

#[test]
fn switch_settings_appear_for_expanders_and_transceivers() {
    let res = check(&p(vec![direct(
        50.0,
        vec![d("M1XRFTWM", 2), dn("M1XIN", 4, "Shed")],
    )]));
    let g = res
        .checklist
        .iter()
        .find(|g| g.title == "Addresses to set")
        .unwrap();
    let c = g.items.join("\n");
    assert!(c.contains("zones 49-64; S1 off, S2 off, S3 on, S4 off"));
    assert!(c.contains("primary transceiver; S1 off, S2 on, S3 off, S4 off"));
    assert!(c.contains("Bus Module Enrollment"));
}

// ---- Rust-side additions: formatting and JSON in/out ----

#[test]
fn zero_one_decimal_format_matches_dotnet() {
    assert_eq!(f(0.0), "0");
    assert_eq!(f(150.0), "150");
    assert_eq!(f(1.25), "1.3"); // tie rounds away from zero
    assert_eq!(f(0.35), "0.4"); // .NET rounds the 15-digit value, not the binary expansion
    assert_eq!(f(2.04), "2");
    assert_eq!(f(9.96), "10");
    assert_eq!(f(99.95), "100");
    assert_eq!(f(0.04), "0");
    assert_eq!(f(-5.0), "-5");
    assert_eq!(f(4000.0), "4000");
    assert_eq!(f(1234567.89), "1234567.9");
}

#[test]
fn json_input_is_case_insensitive_and_lenient_like_web_defaults() {
    let body = r#"{"RUNS":[{"Kind":"direct","run":{"lengthFt":"150","devices":[{"model":"Keypad","address":"1","extra":1}]}}],"wirelessGroups":[2]}"#;
    let plan = BusPlan::from_json(body).unwrap();
    assert_eq!(plan.runs[0].run.length_ft, 150.0);
    assert_eq!(plan.runs[0].run.devices[0].address, 1);
    assert_eq!(plan.wireless_groups, vec![2]);
    assert_eq!(BusPlan::from_json("null").unwrap(), BusPlan::default());
    assert!(BusPlan::from_json("{").is_err());
    assert!(BusPlan::from_json(r#"{"runs":[{"run":{"devices":[{"address":1.5}]}}]}"#).is_err());
}

#[test]
fn defaults_apply_for_missing_members() {
    let plan = BusPlan::from_json(r#"{"runs":[{"run":{"devices":[{}]}}]}"#).unwrap();
    assert_eq!(plan.runs[0].kind, "direct");
    assert_eq!(plan.runs[0].feed_ft, 1.0);
    assert_eq!(plan.runs[0].run.devices[0], BusDevice::default());
}

#[test]
fn result_json_uses_camel_case_in_declaration_order() {
    let out = check_json(r#"{"runs":[{"kind":"direct","name":"Bus","run":{"name":"Bus","lengthFt":150,"devices":[{"model":"Keypad","address":1,"name":"Hall"}]}}]}"#).unwrap();
    assert!(out.starts_with("{\"issues\":["));
    assert!(out.contains("\"checklist\":[{\"title\":\"Control\""));
    assert!(out.ends_with(",\"mainBusFt\":150,\"limitFt\":4000,\"currentMa\":0,\"jp3\":true,\"devices\":1,\"errors\":0,\"warnings\":0}") || out.contains("\"mainBusFt\":150,\"limitFt\":4000,\"currentMa\":0,\"jp3\":true,\"devices\":1,\"errors\":0,\"warnings\":0}"));
    assert!(out.contains("\\u2192"));
}

#[test]
fn meta_tables_serialise() {
    assert!(models_json().starts_with("[{\"id\":\"Keypad\",\"label\":\"Keypad (M1KP2, M1KP3, M1KPNAV, ...)\",\"type\":1,\"minAddress\":1,\"maxAddress\":16,\"note\":\"Type 1\"}"));
    assert!(zone_expander_switches_json()
        .starts_with("{\"2\":\"S1 off, S2 on, S3 off, S4 off\",\"3\":"));
}
