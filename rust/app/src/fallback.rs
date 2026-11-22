//! `rust-anything app replay` sets `RUST_ANYTHING_FALLBACK_LOG` and counts the lines written to it. This build never falls back to
//! the C# server, but creating the file tells the replay that it counts fallbacks (and made none).

/// Creates the fallback log named by `RUST_ANYTHING_FALLBACK_LOG`, if any.
pub fn report_fallbacks() {
    if let Some(path) = std::env::var_os("RUST_ANYTHING_FALLBACK_LOG") {
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path);
    }
}
