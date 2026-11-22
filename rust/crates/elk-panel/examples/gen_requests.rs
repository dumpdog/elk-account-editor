//! Writes the request bodies of the golden scenarios into `tests/fixtures/requests` (the port is the placeholder `@PORT@`, replaced when a request is sent).
//! Run from the crate folder: `cargo run --example gen_requests`.

use elk_account::{decode_latin1, AccountFile, AccountModel};
use std::path::PathBuf;

fn request(model: &AccountModel, reset: bool, local: bool) -> String {
    format!("{{\"host\":\"127.0.0.1\",\"port\":@PORT@,\"code\":\"246801\",\"plain\":true,\"model\":{},\"resetMissing\":{reset},\"sendLocalCode\":{local}}}", model.to_json())
}

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out = root.join("tests/fixtures/requests");
    std::fs::create_dir_all(&out).unwrap();
    let sample = |n: &str| AccountFile::parse(&decode_latin1(&std::fs::read(root.join("../../../sample").join(n)).unwrap())).unwrap();
    let write = |name: &str, body: String| std::fs::write(out.join(format!("{name}.json")), body).unwrap();

    let comm = sample("Sample Communicator");
    let clean = AccountModel::from(&comm).unwrap();
    write("comm_clean", request(&clean, false, false));

    let mut edit = clean.clone();
    edit.zones[0].name = "Edited Zone".into();
    edit.phones[0].number = "5550000".into();
    edit.comm.as_mut().unwrap().zones[0].codes[0] = 0;
    edit.areas[0].name = "Edited Area".into();
    edit.keypads[0].name = "Edited Pad".into();
    write("comm_edit", request(&edit, false, false));
    write("comm_edit_reset", request(&edit, true, false));

    let mut globals = clean.clone();
    let g = globals.globals.as_mut().unwrap();
    g[0] ^= 1;
    g[17] ^= 1; // the local programming code
    write("comm_edit_globals", request(&globals, false, true));
    write("comm_edit_globals_nolocal", request(&globals, false, false));

    let mut bad = clean.clone();
    bad.zones[0].n = 209;
    write("comm_bad_zone", request(&bad, false, false));
    write("comm_wrongcode", request(&clean, false, false).replace("\"code\":\"246801\"", "\"code\":\"000000\""));
    write("comm_port1", request(&clean, false, false).replace("@PORT@", "1"));

    let tmpl = AccountFile::parse(&elk_panel::template_text()).unwrap();
    let tclean = AccountModel::from(&tmpl).unwrap();
    write("tmpl_reset", request(&tclean, true, false));
    write("tmpl_clean", request(&tclean, false, false));
}
