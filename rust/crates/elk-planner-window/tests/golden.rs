//! Golden checks: each `tests/fixtures/databus/NAME.plan.json` was POSTed to the running C# server
//! (`/api/databus/check`) and the response body saved as `NAME.expected.json`. The Rust output must equal it byte for byte.

use elk_planner_window::data_bus_plan::{check_json, models_json, zone_expander_switches_json};
use std::fs;
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("databus")
}

#[test]
fn every_fixture_matches_the_csharp_server_byte_for_byte() {
    let mut n = 0;
    let mut names: Vec<_> = fs::read_dir(dir())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    for name in names.iter().filter(|n| n.ends_with(".plan.json")) {
        let stem = name.trim_end_matches(".plan.json");
        let plan = fs::read_to_string(dir().join(name)).unwrap();
        let expected = fs::read_to_string(dir().join(format!("{}.expected.json", stem))).unwrap();
        let got = check_json(&plan).unwrap_or_else(|e| panic!("{}: {}", stem, e));
        assert_eq!(got, expected, "fixture {}", stem);
        n += 1;
    }
    assert!(n >= 15, "only {} fixtures found", n);
}

#[test]
fn meta_tables_match_api_meta() {
    let meta = fs::read_to_string(dir().join("_meta.expected.json")).unwrap();
    assert!(
        meta.contains(&format!("\"busModels\":{}", models_json())),
        "busModels differ"
    );
    assert!(
        meta.contains(&format!(
            "\"zoneExpanderSwitches\":{}",
            zone_expander_switches_json()
        )),
        "zoneExpanderSwitches differ"
    );
}
