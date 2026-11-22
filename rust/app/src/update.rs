//! "Check for updates" and in-place update (Rust editor only, ADR-016).
//!
//! The release feed is GitHub's `releases/latest` JSON (or any server that answers the same shape). Nothing happens without a click: the page
//! asks for a check, shows the release notes, and only an explicit "Install" downloads the zip for this platform, verifies its SHA-256 against the
//! release's `SHA256SUMS`, unpacks it into a staging folder beside the program, swaps the files (keeping a backup, with a rollback when anything fails)
//! and restarts. The data folder (accounts library) is never touched. No telemetry: the only request is the one to the release feed and its files.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use sha2::{Digest, Sha256};

/// The version of this build (`rust/app/Cargo.toml`).
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
/// Where releases are looked up unless `ELKEDITOR_UPDATE_URL` or `settings.json` says otherwise.
pub const DEFAULT_URL: &str =
    "https://api.github.com/repos/dumpdog/elk-account-editor-rust/releases/latest";
const SUMS_NAME: &str = "SHA256SUMS";
const MAX_FEED: u64 = 2 * 1024 * 1024;
const MAX_DOWNLOAD: u64 = 300 * 1024 * 1024;
const MAX_UNPACKED: u64 = 600 * 1024 * 1024;

// ---- versions ----------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub nums: [u64; 3],
    pub pre: Option<String>,
}

/// `1.2.3`, `v1.2.3`, `1.2`, `1.2.3-rc.1`, `1.2.3+build` (the build part is ignored).
pub fn parse_version(s: &str) -> Option<Version> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let s = s.split('+').next()?;
    let (core, pre) = match s.split_once('-') {
        Some((c, p)) if !p.is_empty() => (c, Some(p.to_string())),
        Some(_) => return None,
        None => (s, None),
    };
    let mut nums = [0u64; 3];
    let parts: Vec<&str> = core.split('.').collect();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    for (i, p) in parts.iter().enumerate() {
        if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        nums[i] = p.parse().ok()?;
    }
    Some(Version { nums, pre })
}

fn cmp_pre(a: &str, b: &str) -> Ordering {
    let (xa, xb): (Vec<&str>, Vec<&str>) = (a.split('.').collect(), b.split('.').collect());
    for (p, q) in xa.iter().zip(xb.iter()) {
        let o = match (p.parse::<u64>(), q.parse::<u64>()) {
            (Ok(m), Ok(n)) => m.cmp(&n),
            (Ok(_), Err(_)) => Ordering::Less,
            (Err(_), Ok(_)) => Ordering::Greater,
            (Err(_), Err(_)) => p.cmp(q),
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    xa.len().cmp(&xb.len())
}

pub fn compare(a: &Version, b: &Version) -> Ordering {
    a.nums.cmp(&b.nums).then_with(|| match (&a.pre, &b.pre) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => cmp_pre(x, y),
    })
}

/// True when `latest` is a higher version than `current`; `None` when either cannot be read.
pub fn is_newer(current: &str, latest: &str) -> Option<bool> {
    Some(compare(&parse_version(latest)?, &parse_version(current)?) == Ordering::Greater)
}

// ---- the release feed -------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub tag: String,
    pub name: String,
    pub notes: String,
    pub page: String,
    pub assets: Vec<Asset>,
}

pub fn parse_release(json: &str) -> Result<Release, String> {
    let v: Value = serde_json::from_str(json)
        .map_err(|e| format!("The release feed is not valid JSON ({e})."))?;
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let tag = s("tag_name");
    if tag.is_empty() {
        return Err("The release feed has no tag_name.".into());
    }
    let assets = v
        .get("assets")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    Some(Asset {
                        name: x.get("name")?.as_str()?.to_string(),
                        url: x.get("browser_download_url")?.as_str()?.to_string(),
                        size: x.get("size").and_then(Value::as_u64).unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Release {
        name: if s("name").is_empty() {
            tag.clone()
        } else {
            s("name")
        },
        tag,
        notes: s("body"),
        page: s("html_url"),
        assets,
    })
}

/// The zip names this build can use, best first (`std::env::consts` values).
pub fn asset_names(os: &str, arch: &str) -> Vec<&'static str> {
    match (os, arch) {
        ("windows", _) => vec!["ElkAccountEditor-win-x64.zip"],
        ("macos", "aarch64") => vec![
            "ElkAccountEditor-mac-arm64.zip",
            "ElkAccountEditor-mac-universal2.zip",
        ],
        ("macos", _) => vec!["ElkAccountEditor-mac-universal2.zip"],
        _ => vec![],
    }
}

pub fn pick_asset<'a>(names: &[&str], assets: &'a [Asset]) -> Option<&'a Asset> {
    names
        .iter()
        .find_map(|n| assets.iter().find(|a| a.name == *n))
}

// ---- checksums -------------------------------------------------------------------------------------------------------

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `sha256sum` output: `<64 hex>  <name>` or `<hex> *<name>`; names are matched without any folder part.
pub fn parse_sums(text: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        let Some((h, rest)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let name = rest.trim().trim_start_matches('*');
        let name = name.rsplit(['/', '\\']).next().unwrap_or(name);
        if h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()) && !name.is_empty() {
            m.insert(name.to_string(), h.to_ascii_lowercase());
        }
    }
    m
}

/// Refuses anything that is not exactly the file the release lists.
pub fn verify(bytes: &[u8], name: &str, sums: &HashMap<String, String>) -> Result<(), String> {
    let Some(want) = sums.get(name) else {
        return Err(format!(
            "{SUMS_NAME} has no entry for {name}; the download was refused."
        ));
    };
    let got = sha256_hex(bytes);
    if &got != want {
        return Err(format!(
            "The checksum of {name} does not match {SUMS_NAME} (expected {want}, got {got}); the download was refused and nothing was installed."
        ));
    }
    Ok(())
}

// ---- HTTP ------------------------------------------------------------------------------------------------------------

#[derive(Debug)]
pub enum FetchError {
    Status(u16),
    Other(String),
}

fn host_is_loopback(url: &str) -> bool {
    let rest = url.split("://").nth(1).unwrap_or("");
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit('@').next().unwrap_or(host);
    let h = if host.starts_with('[') {
        host.split(']').next().unwrap_or("").trim_start_matches('[')
    } else {
        host.split(':').next().unwrap_or("")
    };
    matches!(h, "127.0.0.1" | "localhost" | "::1")
}

/// https always; plain http only to this computer (a test server).
pub fn check_url(url: &str) -> Result<(), String> {
    if url.starts_with("https://") || (url.starts_with("http://") && host_is_loopback(url)) {
        Ok(())
    } else {
        Err(format!(
            "The update address must start with https:// (got {url})."
        ))
    }
}

fn agent() -> Result<ureq::Agent, String> {
    let tls = native_tls::TlsConnector::new().map_err(|e| format!("TLS is not available: {e}"))?;
    Ok(ureq::AgentBuilder::new()
        .tls_connector(Arc::new(tls))
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(30))
        .redirects(5)
        .user_agent(&format!("ElkAccountEditor/{CURRENT} (update check)"))
        .build())
}

fn get(url: &str, accept: &str, max: u64) -> Result<Vec<u8>, FetchError> {
    check_url(url).map_err(FetchError::Other)?;
    let a = agent().map_err(FetchError::Other)?;
    match a.get(url).set("Accept", accept).call() {
        Ok(r) => {
            let mut buf = Vec::new();
            r.into_reader()
                .take(max + 1)
                .read_to_end(&mut buf)
                .map_err(|e| FetchError::Other(e.to_string()))?;
            if buf.len() as u64 > max {
                return Err(FetchError::Other(format!(
                    "The download is larger than {} MB; it was refused.",
                    max / 1024 / 1024
                )));
            }
            Ok(buf)
        }
        Err(ureq::Error::Status(c, _)) => Err(FetchError::Status(c)),
        Err(e) => Err(FetchError::Other(e.to_string())),
    }
}

/// The words the page shows for a failed request.
pub fn friendly(e: &FetchError, what: &str) -> String {
    match e {
        FetchError::Status(404) => format!(
            "Could not {what}: not found (HTTP 404). The repository is probably not public yet, or it has no release."
        ),
        FetchError::Status(c @ (403 | 429)) => format!(
            "Could not {what}: the server refused the request (HTTP {c}); too many requests, try again later."
        ),
        FetchError::Status(c) => format!("Could not {what}: HTTP {c}."),
        FetchError::Other(m) => format!("Could not {what}: {m}"),
    }
}

pub fn fetch_release(url: &str) -> Result<Release, String> {
    let body = get(url, "application/vnd.github+json", MAX_FEED)
        .map_err(|e| friendly(&e, "check for updates"))?;
    parse_release(&String::from_utf8_lossy(&body))
}

// ---- what a check answers --------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub current: String,
    pub latest: String,
    pub newer: bool,
    pub release: Release,
    pub asset: Option<Asset>,
    pub has_sums: bool,
}

pub fn check(url: &str, names: &[&str]) -> Result<Check, String> {
    let release = fetch_release(url)?;
    let newer = is_newer(CURRENT, &release.tag).ok_or_else(|| {
        format!(
            "The release tag \"{}\" is not a version number.",
            release.tag
        )
    })?;
    let asset = pick_asset(names, &release.assets).cloned();
    let has_sums = release.assets.iter().any(|a| a.name == SUMS_NAME);
    Ok(Check {
        current: CURRENT.to_string(),
        latest: release.tag.trim_start_matches(['v', 'V']).to_string(),
        newer,
        release,
        asset,
        has_sums,
    })
}

// ---- staging, swap and rollback ---------------------------------------------------------------------------------------

/// Where the running program lives.
#[derive(Debug, Clone)]
pub struct Install {
    /// The running executable.
    pub exe: PathBuf,
    /// The `.app` bundle that contains it (macOS), else `None`.
    pub bundle: Option<PathBuf>,
    /// The accounts library folder; no part of an update may touch it.
    pub data_dir: Option<PathBuf>,
}

impl Install {
    pub fn detect(exe: &Path, data_dir: Option<&Path>) -> Install {
        let bundle = exe
            .ancestors()
            .find(|a| {
                a.file_name()
                    .is_some_and(|n| n.to_string_lossy().to_ascii_lowercase().ends_with(".app"))
            })
            .map(Path::to_path_buf);
        Install {
            exe: exe.to_path_buf(),
            bundle,
            data_dir: data_dir.map(Path::to_path_buf),
        }
    }

    /// The folder that holds what is replaced (the folder of the exe, or the one that contains the `.app`).
    pub fn base(&self) -> PathBuf {
        self.bundle
            .as_ref()
            .unwrap_or(&self.exe)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    }

    pub fn staging(&self) -> PathBuf {
        self.base().join("ElkEditor update staging")
    }
}

/// One replacement: `new` takes the place of `target`, the old one is kept as `backup`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub target: PathBuf,
    pub new: PathBuf,
    pub backup: PathBuf,
}

fn remove_any(p: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(p) {
        Err(_) => Ok(()),
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(p),
        Ok(_) => std::fs::remove_file(p),
    }
}

fn suffixed(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

/// Unpacks a release zip into `dest` (created). Paths that leave `dest` are refused.
pub fn unpack_zip(bytes: &[u8], dest: &Path) -> Result<(), String> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| format!("The download is not a zip file ({e})."))?;
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let mut total = 0u64;
    for i in 0..z.len() {
        let mut f = z.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = f.enclosed_name() else {
            return Err(format!(
                "The zip holds an unsafe path ({}); it was refused.",
                f.name()
            ));
        };
        let out = dest.join(rel);
        if f.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        total += f.size();
        if total > MAX_UNPACKED {
            return Err("The zip unpacks to more than 600 MB; it was refused.".into());
        }
        if let Some(p) = out.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        let mut o = std::fs::File::create(&out).map_err(|e| e.to_string())?;
        std::io::copy(&mut f, &mut o).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        if let Some(mode) = f.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode & 0o777));
        }
    }
    Ok(())
}

/// The folder that holds the files of the release: the single folder inside `dir` (our zips have one), or `dir` itself.
pub fn payload_root(dir: &Path) -> PathBuf {
    let entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|r| r.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    match entries.as_slice() {
        [one]
            if one.is_dir()
                && !one.file_name().is_some_and(|n| {
                    n.to_string_lossy().to_ascii_lowercase().ends_with(".app")
                }) =>
        {
            one.clone()
        }
        _ => dir.to_path_buf(),
    }
}

/// What to replace. macOS bundle: the whole `.app`. Otherwise the executable (whatever it is called now) and `elk` / `README.txt` where they exist.
pub fn plan(inst: &Install, payload: &Path) -> Result<Vec<Item>, String> {
    let mut items = Vec::new();
    if let Some(app) = &inst.bundle {
        let new = std::fs::read_dir(payload)
            .map_err(|e| e.to_string())?
            .flatten()
            .map(|e| e.path())
            .find(|p| {
                p.is_dir()
                    && p.file_name()
                        .is_some_and(|n| n.to_string_lossy().to_ascii_lowercase().ends_with(".app"))
            })
            .ok_or("The release has no .app bundle.")?;
        items.push(Item {
            target: app.clone(),
            new,
            backup: suffixed(app, ".previous"),
        });
        let base = inst.base();
        for name in ["README.txt", "Start in browser.command"] {
            let (t, n) = (base.join(name), payload.join(name));
            if t.is_file() && n.is_file() {
                items.push(Item {
                    backup: suffixed(&t, ".old"),
                    target: t,
                    new: n,
                });
            }
        }
    } else {
        let new_exe = [
            "ElkAccountEditor.exe",
            "elkaccounteditor.exe",
            "ElkAccountEditor",
            "elkaccounteditor",
        ]
        .iter()
        .map(|n| payload.join(n))
        .find(|p| p.is_file())
        .ok_or("The release has no program file.")?;
        items.push(Item {
            backup: suffixed(&inst.exe, ".old"),
            target: inst.exe.clone(),
            new: new_exe,
        });
        let base = inst.base();
        for name in ["elk.exe", "elk", "README.txt"] {
            let (t, n) = (base.join(name), payload.join(name));
            if t.is_file() && n.is_file() {
                items.push(Item {
                    backup: suffixed(&t, ".old"),
                    target: t,
                    new: n,
                });
            }
        }
    }
    if let Some(d) = &inst.data_dir {
        for it in &items {
            if it.target.starts_with(d) || d.starts_with(&it.target) {
                return Err(format!(
                    "{} is inside or around the library data folder; the update refuses to touch it.",
                    it.target.display()
                ));
            }
        }
    }
    Ok(items)
}

fn swap_one(it: &Item) -> Result<(), String> {
    remove_any(&it.backup).map_err(|e| {
        format!(
            "Could not clear the old backup {}: {e}",
            it.backup.display()
        )
    })?;
    std::fs::rename(&it.target, &it.backup)
        .map_err(|e| format!("Could not move {} aside: {e}", it.target.display()))?;
    if let Err(e) = std::fs::rename(&it.new, &it.target) {
        let _ = std::fs::rename(&it.backup, &it.target);
        return Err(format!(
            "Could not put the new {} in place: {e}",
            it.target.display()
        ));
    }
    Ok(())
}

fn undo_one(it: &Item) {
    let _ = remove_any(&it.target);
    let _ = std::fs::rename(&it.backup, &it.target);
}

/// Replaces every item. When one fails, the ones already replaced are put back and the error says what happened.
pub fn swap_all(items: &[Item]) -> Result<(), String> {
    let mut done: Vec<&Item> = Vec::new();
    for it in items {
        if let Err(e) = swap_one(it) {
            for d in done.iter().rev() {
                undo_one(d);
            }
            return Err(format!("{e}. Everything was put back as it was."));
        }
        done.push(it);
    }
    Ok(())
}

/// A folder the program can write to (a read-only disk image or a translocated app cannot be updated in place).
pub fn check_writable(dir: &Path) -> Result<(), String> {
    let probe = dir.join(format!(".update-test-{}", std::process::id()));
    match std::fs::write(&probe, b"x") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            Ok(())
        }
        Err(e) => Err(format!(
            "The folder {} cannot be written ({e}). The app may be running from a disk image, from a quarantined download or from a protected folder: move it to a normal folder (for example Applications), start it from there, and try again, or download the new version by hand.",
            dir.display()
        )),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub version: String,
    /// The program to start again.
    pub restart: PathBuf,
    pub backups: Vec<PathBuf>,
}

/// Downloads, verifies and installs the release `url` points to. `names` are the zip names for this platform. The staging folder is removed
/// whatever happens. Nothing outside the program's own files is touched.
pub fn install(url: &str, names: &[&str], inst: &Install) -> Result<Outcome, String> {
    let c = check(url, names)?;
    if !c.newer {
        return Err(format!(
            "You already have version {} (the newest is {}).",
            c.current, c.latest
        ));
    }
    let asset = c.asset.clone().ok_or_else(|| {
        format!(
            "The release {} has no download for this computer ({}).",
            c.release.tag,
            names.join(" or ")
        )
    })?;
    let sums_asset = c.release.assets.iter().find(|a| a.name == SUMS_NAME).ok_or_else(|| format!("The release has no {SUMS_NAME} file, so the download cannot be checked; nothing was installed."))?;
    check_writable(&inst.base())?;
    let sums_text = get(&sums_asset.url, "text/plain", MAX_FEED)
        .map_err(|e| friendly(&e, &format!("download {SUMS_NAME}")))?;
    let sums = parse_sums(&String::from_utf8_lossy(&sums_text));
    let zip = get(&asset.url, "application/octet-stream", MAX_DOWNLOAD)
        .map_err(|e| friendly(&e, &format!("download {}", asset.name)))?;
    verify(&zip, &asset.name, &sums)?;
    install_zip(&zip, inst, &c.latest)
}

/// The part of [`install`] after the download was verified.
pub fn install_zip(zip: &[u8], inst: &Install, version: &str) -> Result<Outcome, String> {
    let stage = inst.staging();
    let _ = remove_any(&stage);
    let result = (|| {
        unpack_zip(zip, &stage)?;
        let items = plan(inst, &payload_root(&stage))?;
        swap_all(&items)?;
        let restart = items
            .first()
            .map(|i| i.target.clone())
            .unwrap_or_else(|| inst.exe.clone());
        Ok(Outcome {
            version: version.to_string(),
            restart,
            backups: items.iter().map(|i| i.backup.clone()).collect(),
        })
    })();
    let _ = remove_any(&stage);
    result
}

/// Removes `*.old` files left by an earlier update (a running exe cannot be deleted on Windows, so this runs at the next start).
pub fn cleanup_old(inst: &Install) {
    let base = inst.base();
    for name in ["elk.exe", "elk", "README.txt"] {
        let _ = remove_any(&suffixed(&base.join(name), ".old"));
    }
    if inst.bundle.is_none() {
        let _ = remove_any(&suffixed(&inst.exe, ".old"));
    }
    let _ = remove_any(&inst.staging());
}

/// Starts the new program and ends this one shortly afterwards (on a thread, so the answer to the page goes out first). The new process waits for
/// the local port to be free again (`ELKEDITOR_RESTART`) so the page keeps its address and its stored drafts.
pub fn restart_soon(program: PathBuf, args: Vec<String>) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1200));
        let _ = std::process::Command::new(&program)
            .args(&args)
            .env("ELKEDITOR_RESTART", "1")
            .spawn();
        std::thread::sleep(Duration::from_millis(300));
        std::process::exit(0);
    });
}

#[cfg(test)]
pub(crate) mod tests;
