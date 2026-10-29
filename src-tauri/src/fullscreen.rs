//! Detects when another application owns a true fullscreen surface (games,
//! video players, F11 browsers) so the island can tuck itself away.

use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetShellWindow, GetWindowRect, IsZoomed,
};

pub fn start(app: AppHandle) {
    let own = app
        .get_webview_window("main")
        .and_then(|win| win.hwnd().ok())
        .map(|raw| raw.0 as isize);

    let Some(own) = own else {
        return;
    };

    std::thread::Builder::new()
        .name("island-fullscreen".into())
        .spawn(move || {
            let own = HWND(own as *mut core::ffi::c_void);
            let mut last: Option<bool> = None;
            loop {
                let full = foreground_is_fullscreen(own);
                if last != Some(full) {
                    last = Some(full);
                    let _ = app.emit("island://fullscreen", full);
                }
                std::thread::sleep(Duration::from_millis(1200));
            }
        })
        .ok();
}

fn foreground_is_fullscreen(own: HWND) -> bool {
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.0.is_null() || foreground == own || foreground == GetShellWindow() {
            return false;
        }

        let mut rect = RECT::default();
        if GetWindowRect(foreground, &mut rect).is_err() {
            return false;
        }

        let monitor = MonitorFromWindow(foreground, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return false;
        }

        let screen = info.rcMonitor;
        let covers = rect.left <= screen.left + 1
            && rect.top <= screen.top + 1
            && rect.right >= screen.right - 1
            && rect.bottom >= screen.bottom - 1;

        // A maximized window also fills the work area on some setups; it keeps
        // its caption and should not hide the island.
        covers && !IsZoomed(foreground).as_bool()
    }
}
