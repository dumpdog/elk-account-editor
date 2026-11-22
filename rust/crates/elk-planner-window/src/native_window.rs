//! The native window: wry (WebView2 on Windows, WebKit on macOS) inside a tao window, replacing Photino.NET.
//!
//! `index.html` is unchanged. It looks for `window.external.sendMessage` / `receiveMessage` (the Photino API), so an
//! initialisation script defines them on top of wry's `window.ipc.postMessage`, and replies come back through
//! `evaluate_script`.

use crate::bridge::Dialogs;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub const TITLE: &str = "Elk account editor";
pub const INITIAL_SIZE: (f64, f64) = (1500.0, 950.0);
pub const MIN_SIZE: (f64, f64) = (900.0, 600.0);

/// Defines `window.external.sendMessage(json)` and `window.external.receiveMessage(cb)` for the page.
pub const INIT_SCRIPT: &str = r#"(function () {
  if (window.__elkBridge) return;
  window.__elkBridge = true;
  var listeners = [];
  var api = {
    sendMessage: function (m) { window.ipc.postMessage(String(m)); },
    receiveMessage: function (cb) { if (typeof cb === 'function') listeners.push(cb); }
  };
  window.__elkReceive = function (raw) {
    listeners.slice().forEach(function (cb) { try { cb(raw); } catch (e) { console.error(e); } });
  };
  // WebView2 allows redefining the property at once. WebKit (macOS) has its own native `window.external`
  // (the search provider stub), so fall back to deleting it, assigning it, and finally patching the existing object.
  function define() {
    try { Object.defineProperty(window, 'external', { value: api, configurable: true, writable: true }); } catch (e) { }
  }
  define();
  if (window.external !== api) { try { delete window.external; } catch (e) { } define(); }
  if (window.external !== api) { try { window.external = api; } catch (e) { } }
  if (window.external !== api) {
    try { window.external.sendMessage = api.sendMessage; window.external.receiveMessage = api.receiveMessage; } catch (e) { }
  }
})();"#;

/// The script that delivers one reply (a JSON text) to the page.
pub fn deliver_script(reply_json: &str) -> String {
    let mut lit = String::new();
    crate::jsonw::push_str(&mut lit, reply_json); // pure ASCII, valid as a JS string literal
    format!("window.__elkReceive({})", lit)
}

/// Opens the window on `url` and blocks until it is closed. Must be called on the main thread.
/// An error (for example no WebView2 runtime) means no window was shown, so the caller can fall back to the browser.
pub fn run(url: &str) -> Result<()> {
    #[cfg(any(windows, target_os = "macos"))]
    {
        run_with_dialogs(url, Box::new(crate::dialogs::RfdDialogs))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = url;
        Err("the native window is only available on Windows and macOS".into())
    }
}

/// Like [`run`], with the dialogs supplied (tests and the example use fakes).
#[cfg(any(windows, target_os = "macos"))]
pub fn run_with_dialogs(url: &str, dialogs: Box<dyn Dialogs>) -> Result<()> {
    use crate::bridge::handle_message;
    use tao::dpi::{LogicalSize, PhysicalPosition};
    use tao::event::{Event, WindowEvent};
    use tao::event_loop::{ControlFlow, EventLoopBuilder};
    use tao::platform::run_return::EventLoopExtRunReturn;
    use tao::window::WindowBuilder;
    use wry::{WebContext, WebViewBuilder};

    enum UserEvent {
        Reply(String),
        #[cfg(target_os = "macos")]
        Quit,
    }

    let mut event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    // Keep the menu alive for as long as the loop runs.
    #[cfg(target_os = "macos")]
    let _menu = {
        let quit_proxy = proxy.clone();
        macos_menu::install(move || {
            let _ = quit_proxy.send_event(UserEvent::Quit);
        })
    };
    let window = WindowBuilder::new()
        .with_title(TITLE)
        .with_inner_size(LogicalSize::new(INITIAL_SIZE.0, INITIAL_SIZE.1))
        .with_min_inner_size(LogicalSize::new(MIN_SIZE.0, MIN_SIZE.1))
        .with_resizable(true)
        .build(&event_loop)?;

    // Center on the monitor the window is on
    if let Some(m) = window
        .current_monitor()
        .or_else(|| window.primary_monitor())
    {
        let (ms, mp, ws) = (m.size(), m.position(), window.outer_size());
        let x = mp.x + (ms.width as i32 - ws.width as i32) / 2;
        let y = mp.y + (ms.height as i32 - ws.height as i32) / 2;
        window.set_outer_position(PhysicalPosition::new(x.max(mp.x), y.max(mp.y)));
    }

    // WebView2 keeps its profile (localStorage: drafts, remembered panel address) here, so it survives between runs.
    let mut context =
        WebContext::new(dirs::data_local_dir().map(|d| d.join("ElkAccountEditor").join("webview")));
    let webview = WebViewBuilder::new_with_web_context(&mut context)
        .with_url(url)
        .with_initialization_script(INIT_SCRIPT)
        .with_ipc_handler(move |req| {
            // File dialogs are modal, so doing them here (on the UI thread) matches what Photino did.
            if let Some(json) = handle_message(req.body(), dialogs.as_ref()).to_json() {
                let _ = proxy.send_event(UserEvent::Reply(json));
            }
        })
        .build(&window)?;

    event_loop.run_return(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(UserEvent::Reply(json)) => {
                let _ = webview.evaluate_script(&deliver_script(&json));
            }
            #[cfg(target_os = "macos")]
            Event::UserEvent(UserEvent::Quit) => *flow = ControlFlow::Exit,
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *flow = ControlFlow::Exit,
            _ => {}
        }
    });
    Ok(())
}

/// The macOS menu bar. WKWebView only handles Cmd+C/V/X/A/Z through the Edit menu's first-responder actions, and a bare
/// tao app has no menu at all (so no Cmd+Q / Cmd+W either).
#[cfg(target_os = "macos")]
mod macos_menu {
    use muda::accelerator::{Accelerator, Code, Modifiers};
    use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};

    /// Builds and installs the menu; `on_quit` runs (on the main thread) when "Quit" or Cmd+Q is chosen,
    /// so the event loop ends normally and the caller's cleanup (panel logout) still runs.
    pub fn install(on_quit: impl Fn() + Send + Sync + 'static) -> Option<Menu> {
        let menu = Menu::new();
        let quit = MenuItem::new(
            "Quit Elk account editor",
            true,
            Some(Accelerator::new(Modifiers::META, Code::KeyQ)),
        );
        let quit_id = quit.id().clone();
        let app = Submenu::new("Elk account editor", true);
        let edit = Submenu::new("Edit", true);
        let window = Submenu::new("Window", true);
        let built = app
            .append_items(&[
                &PredefinedMenuItem::hide(None),
                &PredefinedMenuItem::hide_others(None),
                &PredefinedMenuItem::show_all(None),
                &PredefinedMenuItem::separator(),
                &quit,
            ])
            .and_then(|_| {
                edit.append_items(&[
                    &PredefinedMenuItem::undo(None),
                    &PredefinedMenuItem::redo(None),
                    &PredefinedMenuItem::separator(),
                    &PredefinedMenuItem::cut(None),
                    &PredefinedMenuItem::copy(None),
                    &PredefinedMenuItem::paste(None),
                    &PredefinedMenuItem::select_all(None),
                ])
            })
            .and_then(|_| {
                window.append_items(&[
                    &PredefinedMenuItem::minimize(None),
                    &PredefinedMenuItem::close_window(None),
                ])
            })
            .and_then(|_| menu.append_items(&[&app, &edit, &window]));
        if built.is_err() {
            return None;
        }
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            if e.id == quit_id {
                on_quit();
            }
        }));
        menu.init_for_nsapp();
        Some(menu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deliver_script_wraps_the_json_as_a_string_literal() {
        assert_eq!(
            deliver_script(r#"{"cmd":"saved","path":"C:\\x"}"#),
            r#"window.__elkReceive("{\u0022cmd\u0022:\u0022saved\u0022,\u0022path\u0022:\u0022C:\\\\x\u0022}")"#
        );
    }

    #[test]
    fn init_script_defines_the_photino_api() {
        assert!(
            INIT_SCRIPT.contains("sendMessage")
                && INIT_SCRIPT.contains("receiveMessage")
                && INIT_SCRIPT.contains("ipc.postMessage")
        );
    }
}
