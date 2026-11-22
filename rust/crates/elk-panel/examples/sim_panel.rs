//! A plain-mode simulated panel on a loopback port, for the golden run against the C# editor.
//!
//! `sim_panel <setup> <sample|@template>` fills the panel (setups: `seed`, `defaults`, `pull`; see `elk_panel::testkit::setup`), prints `PORT <n>` and
//! serves until it is killed. The RP access code is 246801.

use elk_account::{decode_latin1, AccountFile};
use elk_core::simulated_panel::SimulatedPanel;
use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: sim_panel <setup> <sample file|@template>");
        std::process::exit(2);
    }
    let text = if args[2] == "@template" { elk_panel::template_text() } else { decode_latin1(&std::fs::read(&args[2]).expect("read sample")) };
    let f = AccountFile::parse(&text).expect("parse sample");
    let panel = SimulatedPanel::new_plain("246801");
    elk_panel::testkit::setup(&args[1], &panel, &f).expect("setup");
    println!("PORT {}", panel.port());
    let _ = std::io::stdout().flush();
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
