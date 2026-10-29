//! Per-region click-through.
//!
//! The island window is a large transparent rectangle, but only the island
//! itself should receive mouse input. Answering `WM_NCHITTEST` on the top-level
//! HWND is not enough: WebView2 hosts its own child window
//! (`Chrome_RenderWidgetHostHWND`) that wins hit-testing before the parent is
//! ever asked. We therefore poll the cursor and toggle `WS_EX_TRANSPARENT` +
//! `WS_EX_LAYERED` on the whole window so Windows skips it entirely outside the
//! island. The `WM_NCHITTEST` subclass stays as a second line of defence for
//! the areas the root HWND does own.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

#[cfg(windows)]
use tauri::WebviewWindow;
#[cfg(windows)]
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
#[cfg(windows)]
use windows::Win32::Graphics::Gdi::{ClientToScreen, ScreenToClient};
#[cfg(windows)]
use windows::Win32::UI::HiDpi::GetDpiForWindow;
#[cfg(windows)]
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
#[cfg(windows)]
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE,
    SWP_ASYNCWINDOWPOS, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    WM_NCHITTEST, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};

const HT_CLIENT: isize = 1;
const HT_TRANSPARENT: isize = -1;

/// Island bounds in CSS pixels, relative to the webview viewport.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default)]
pub struct HitRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub radius: f64,
}

impl HitRect {
    pub fn contains(&self, px: f64, py: f64) -> bool {
        if self.width <= 0.0 || self.height <= 0.0 {
            return false;
        }
        let (x, y) = (px, py);
        if x < self.x || y < self.y || x > self.x + self.width || y > self.y + self.height {
            return false;
        }
        let r = self
            .radius
            .min(self.width / 2.0)
            .min(self.height / 2.0)
            .max(0.0);
        if r <= 0.0 {
            return true;
        }
        // Distance to the nearest point of the inner (non-corner) rectangle.
        let cx = x.clamp(self.x + r, self.x + self.width - r);
        let cy = y.clamp(self.y + r, self.y + self.height - r);
        let dx = x - cx;
        let dy = y - cy;
        dx * dx + dy * dy <= r * r
    }
}

static HIT_RECT: Mutex<HitRect> = Mutex::new(HitRect {
    x: 0.0,
    y: 0.0,
    width: 0.0,
    height: 0.0,
    radius: 0.0,
});

/// Last applied click-through state; `None` until the first poll decides.
static CLICK_THROUGH: Mutex<Option<bool>> = Mutex::new(None);
static RUNNING: AtomicBool = AtomicBool::new(false);

pub fn set(rect: HitRect) {
    if let Ok(mut guard) = HIT_RECT.lock() {
        *guard = rect;
    }
}

#[cfg(windows)]
pub fn install(hwnd: HWND) {
    unsafe {
        let _ = SetWindowSubclass(hwnd, Some(subclass_proc), 1, 0);
    }
}

/// Starts the cursor poll that keeps the window click-through outside the
/// island. Safe to call once; repeated calls are ignored.
#[cfg(windows)]
pub fn start(win: WebviewWindow) {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let Ok(raw) = win.hwnd() else {
        RUNNING.store(false, Ordering::SeqCst);
        return;
    };
    let hwnd = raw.0 as isize;

    std::thread::Builder::new()
        .name("island-hit-test".into())
        .spawn(move || {
            let hwnd = HWND(hwnd as *mut core::ffi::c_void);
            loop {
                if let Some(ignore) = desired_click_through(hwnd) {
                    let changed = CLICK_THROUGH
                        .lock()
                        .map(|mut state| {
                            if *state == Some(ignore) {
                                return false;
                            }
                            *state = Some(ignore);
                            true
                        })
                        .unwrap_or(false);
                    if changed {
                        apply_click_through(hwnd, ignore);
                    }
                }
                std::thread::sleep(Duration::from_millis(16));
            }
        })
        .ok();
}

#[cfg(not(windows))]
pub fn start(_win: tauri::WebviewWindow) {}

/// Whether the OS cursor currently sits over the island rect. `None` while the
/// UI has not reported a rect yet, so callers can treat it as "unknown".
///
/// The webview cannot be trusted for this: as soon as the cursor leaves the
/// island we turn the window click-through, and no further mouse events (not
/// even `mouseleave`) are delivered to it.
#[cfg(windows)]
pub fn cursor_inside(hwnd: HWND) -> Option<bool> {
    unsafe {
        let rect = HIT_RECT.lock().ok().map(|r| *r)?;
        if rect.width <= 0.0 || rect.height <= 0.0 {
            return None;
        }
        let mut cursor = POINT::default();
        if GetCursorPos(&mut cursor).is_err() {
            return None;
        }
        let mut origin = POINT { x: 0, y: 0 };
        let _ = ClientToScreen(hwnd, &mut origin);
        let scale = (GetDpiForWindow(hwnd) as f64 / 96.0).max(0.25);
        Some(rect.contains(
            (cursor.x - origin.x) as f64 / scale,
            (cursor.y - origin.y) as f64 / scale,
        ))
    }
}

#[cfg(windows)]
fn desired_click_through(hwnd: HWND) -> Option<bool> {
    unsafe {
        // Mid-click: never flip the style, otherwise a press that started on
        // the island (drag, button) would be cancelled halfway through.
        let button = GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16;
        if button & 0x8000 != 0 {
            return None;
        }

        let rect = HIT_RECT.lock().map(|r| *r).unwrap_or_default();
        if rect.width <= 0.0 || rect.height <= 0.0 {
            return Some(true);
        }

        let mut cursor = POINT::default();
        if GetCursorPos(&mut cursor).is_err() {
            return None;
        }

        // Use the client origin: the window is undecorated, but this also
        // survives any future border or DPI virtualisation change.
        let mut origin = POINT { x: 0, y: 0 };
        let _ = ClientToScreen(hwnd, &mut origin);

        let scale = (GetDpiForWindow(hwnd) as f64 / 96.0).max(0.25);
        let css_x = (cursor.x - origin.x) as f64 / scale;
        let css_y = (cursor.y - origin.y) as f64 / scale;
        Some(!rect.contains(css_x, css_y))
    }
}

/// Applies the ex-style bits directly instead of going through tao: tao
/// rebuilds the whole ex-style from its own flags, which would drop the
/// `WS_EX_TOOLWINDOW` bit added by `windowing::prepare_hwnd`.
#[cfg(windows)]
fn apply_click_through(hwnd: HWND, ignore: bool) {
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let bits = (WS_EX_TRANSPARENT.0 | WS_EX_LAYERED.0) as isize;
        let next = if ignore { ex | bits } else { ex & !bits };
        if next == ex {
            return;
        }
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, next | WS_EX_TOOLWINDOW.0 as isize);
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE
                | SWP_NOSIZE
                | SWP_NOZORDER
                | SWP_NOACTIVATE
                | SWP_FRAMECHANGED
                | SWP_ASYNCWINDOWPOS,
        );
    }
}

#[cfg(windows)]
unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if msg == WM_NCHITTEST {
        let rect = HIT_RECT.lock().map(|r| *r).unwrap_or_default();
        if rect.width <= 0.0 || rect.height <= 0.0 {
            return LRESULT(HT_TRANSPARENT);
        }

        let raw = lparam.0 as u32;
        let mut pt = POINT {
            x: (raw & 0xFFFF) as i16 as i32,
            y: ((raw >> 16) & 0xFFFF) as i16 as i32,
        };
        let _ = ScreenToClient(hwnd, &mut pt);

        let scale = (GetDpiForWindow(hwnd) as f64 / 96.0).max(0.25);
        if rect.contains(pt.x as f64 / scale, pt.y as f64 / scale) {
            return LRESULT(HT_CLIENT);
        }
        return LRESULT(HT_TRANSPARENT);
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}
