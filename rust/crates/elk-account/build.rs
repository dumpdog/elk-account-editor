//! Embeds `src/data/event-list-m1g.tsv` (the event table with ElkRP's wording) when the file exists.
//! The public source tree leaves the file out (see `rust/packaging/publish-exclude.txt`); the build then embeds an empty
//! table and the editor falls back to generic wording ("Event 1003") and does not check event numbers.
use std::{env, fs, path::PathBuf};

fn main() {
    let dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let src = dir.join("src/data/event-list-m1g.tsv");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("event-list-m1g.tsv");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/data");
    let bytes = fs::read(&src).unwrap_or_default();
    fs::write(&out, bytes).expect("write event table copy");
}
