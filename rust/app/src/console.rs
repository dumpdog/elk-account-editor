//! Windows release builds are GUI-subsystem programs (no console window when double-clicked). When started from a terminal
//! without usable standard handles, [`init`] attaches to the parent console and re-points stdout / stderr there, so the
//! usual messages still print. Handles that were redirected (pipes, files) are left alone. Fatal errors with no console
//! are shown in a message box and written to a log file ([`install_panic_hook`]). Other platforms: no-ops.

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;

    type Handle = *mut c_void;
    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const STD_ERROR_HANDLE: u32 = -12i32 as u32;
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    const GENERIC_READ_WRITE: u32 = 0xC000_0000;
    const FILE_SHARE_READ_WRITE: u32 = 3;
    const OPEN_EXISTING: u32 = 3;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(n: u32) -> Handle;
        fn SetStdHandle(n: u32, h: Handle) -> i32;
        fn AttachConsole(pid: u32) -> i32;
        fn GetConsoleWindow() -> Handle;
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            sa: *const c_void,
            disposition: u32,
            flags: u32,
            template: Handle,
        ) -> Handle;
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(hwnd: Handle, text: *const u16, caption: *const u16, kind: u32) -> i32;
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn invalid(h: Handle) -> bool {
        h.is_null() || h as isize == -1
    }

    pub fn init() {
        unsafe {
            let out_bad = invalid(GetStdHandle(STD_OUTPUT_HANDLE));
            let err_bad = invalid(GetStdHandle(STD_ERROR_HANDLE));
            if !out_bad && !err_bad {
                return;
            }
            if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
                return; // double-clicked: no parent console
            }
            let name = wide("CONOUT$");
            for (which, bad) in [(STD_OUTPUT_HANDLE, out_bad), (STD_ERROR_HANDLE, err_bad)] {
                if !bad {
                    continue;
                }
                let h = CreateFileW(
                    name.as_ptr(),
                    GENERIC_READ_WRITE,
                    FILE_SHARE_READ_WRITE,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    0,
                    std::ptr::null_mut(),
                );
                if !invalid(h) {
                    SetStdHandle(which, h);
                }
            }
        }
    }

    fn has_output() -> bool {
        unsafe { !invalid(GetStdHandle(STD_ERROR_HANDLE)) || !GetConsoleWindow().is_null() }
    }

    fn log_path() -> Option<std::path::PathBuf> {
        let base = std::env::var_os("ELKEDITOR_DATA")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("LOCALAPPDATA").map(|p| std::path::Path::new(&p).join("ElkEditor"))
            })?;
        std::fs::create_dir_all(&base).ok()?;
        Some(base.join("error.log"))
    }

    pub fn install_panic_hook() {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            default(info);
            if has_output() {
                return;
            }
            let msg = format!(
                "The Elk account editor could not start or stopped unexpectedly.\n\n{info}"
            );
            if let Some(p) = log_path() {
                let _ = std::fs::write(&p, &msg);
            }
            let (t, c) = (wide(&msg), wide("Elk account editor"));
            unsafe {
                MessageBoxW(std::ptr::null_mut(), t.as_ptr(), c.as_ptr(), 0x10);
            }
        }));
    }
}

#[cfg(windows)]
pub use imp::{init, install_panic_hook};

#[cfg(not(windows))]
pub fn init() {}
#[cfg(not(windows))]
pub fn install_panic_hook() {}
