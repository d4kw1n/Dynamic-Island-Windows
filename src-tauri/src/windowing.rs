use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewWindow, WindowEvent,
};

use crate::hit_test;

/// Positions the window at the top-center of the primary monitor.
pub fn place_top_center(win: &WebviewWindow) {
    let scale = win.scale_factor().unwrap_or(1.0);
    let size = win.outer_size().unwrap_or(tauri::PhysicalSize::new(560, 440));

    let monitor = win
        .app_handle()
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| win.primary_monitor().ok().flatten())
        .or_else(|| win.current_monitor().ok().flatten());

    if let Some(monitor) = monitor {
        let x = monitor.position().x + (monitor.size().width as i32 - size.width as i32) / 2;
        let y = monitor.position().y + (5.0 * scale).round() as i32;
        let _ = win.set_position(PhysicalPosition::new(x, y));
    }
}

/// Monitor work area (excluding the taskbar) plus the window DPI scale.
#[cfg(windows)]
fn work_area(win: &WebviewWindow) -> Option<(i32, i32, i32, i32, f64)> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };

    let raw = win.hwnd().ok()?;
    let hwnd = HWND(raw.0 as *mut core::ffi::c_void);
    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return None;
        }
        let work = info.rcWork;
        Some((
            work.left,
            work.top,
            work.right,
            work.bottom,
            win.scale_factor().unwrap_or(1.0),
        ))
    }
}

#[cfg(not(windows))]
fn work_area(_win: &WebviewWindow) -> Option<(i32, i32, i32, i32, f64)> {
    None
}

/// Keeps the pill itself reachable on the target monitor. The window is a much
/// larger transparent rectangle with the island centred inside it, so clamping
/// the window edges would let the pill slide off-screen.
fn clamp_to_monitor(win: &WebviewWindow, x: i32, y: i32) -> (i32, i32) {
    let Some((left, top, right, bottom, scale)) = work_area(win) else {
        return (x, y);
    };

    let size = win.outer_size().unwrap_or(PhysicalSize::new(560, 440));
    let half = size.width as f64 / 2.0;

    // Keep at least ~70 CSS px of the collapsed pill visible on each side.
    let min_center = left as f64 + 70.0 * scale;
    let max_center = right as f64 - 70.0 * scale;
    let center = (x as f64 + half).clamp(min_center, max_center);
    let nx = (center - half).round() as i32;

    // The island starts 5 CSS px below the window top and is ~36 px tall.
    let ny = (y as f64)
        .clamp(top as f64 - 5.0 * scale, bottom as f64 - 41.0 * scale)
        .round() as i32;
    (nx, ny)
}

#[cfg(windows)]
fn prepare_hwnd(win: &WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, HWND_TOPMOST,
        SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WS_EX_TOOLWINDOW,
    };

    if let Ok(raw) = win.hwnd() {
        let hwnd = HWND(raw.0 as *mut core::ffi::c_void);
        unsafe {
            // Keep the overlay out of Alt+Tab and the taskbar.
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(
                hwnd,
                GWL_EXSTYLE,
                ex | (WS_EX_TOOLWINDOW.0 as isize),
            );
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
            hit_test::install(hwnd);
        }
    }
}

#[cfg(windows)]
pub fn show_island(app: &AppHandle) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        ShowWindow, SetWindowPos, HWND_TOPMOST, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOSIZE,
    };

    if let Some(win) = app.get_webview_window("main") {
        if let Ok(raw) = win.hwnd() {
            let hwnd = HWND(raw.0 as *mut core::ffi::c_void);
            unsafe {
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
        }
    }
}

#[cfg(not(windows))]
pub fn show_island(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
    }
}

fn setup_tray(app: &mut App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show island", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &settings, &quit])?;

    let mut builder = TrayIconBuilder::with_id("island-tray")
        .tooltip("Dynamic Island")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_island(app),
            "settings" => {
                show_island(app);
                let _ = app.emit("island://open-settings", ());
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_island(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    // A tray failure should never prevent the island from running.
    let _ = builder.build(app);
    Ok(())
}

pub fn setup(app: &mut App) -> tauri::Result<()> {
    let win = app
        .get_webview_window("main")
        .expect("main window is defined in tauri.conf.json");

    // The configured size is created in window pixels; at a display scale above
    // 100% the webview then sees fewer CSS pixels than the panels need. Scale
    // the window by the monitor factor so the CSS viewport is always 560x440.
    let scale = win.scale_factor().unwrap_or(1.0);
    let target = PhysicalSize::new(
        (560.0 * scale).round() as u32,
        (440.0 * scale).round() as u32,
    );
    if win
        .outer_size()
        .map(|size| size != target)
        .unwrap_or(false)
    {
        let _ = win.set_size(target);
    }

    match crate::store::saved_window_pos(app.handle()) {
        Some((x, y)) => {
            let (x, y) = clamp_to_monitor(&win, x, y);
            let _ = win.set_position(PhysicalPosition::new(x, y));
        }
        None => place_top_center(&win),
    }

    #[cfg(windows)]
    {
        prepare_hwnd(&win);
        hit_test::start(win.clone());
    }

    show_island(app.handle());

    setup_tray(app)?;

    let win_for_events = win.clone();
    win.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = win_for_events.hide();
        }
    });

    Ok(())
}

#[tauri::command]
pub fn set_hit_rect(rect: hit_test::HitRect) {
    hit_test::set(rect);
}

/// Tells the UI whether the cursor is still over the island. Used by the
/// hover-opened panel to collapse once the pointer really leaves (the webview
/// stops receiving mouse events the moment the window turns click-through).
/// Grows the window until the webview reports at least the design viewport
/// (560x440 CSS pixels). WebView2 can map a window to fewer CSS pixels at some
/// display scales; panels then overflow the viewport and the native hit rect no
/// longer lines up with the right-hand buttons.
#[tauri::command]
pub fn fit_island_window(
    app: AppHandle,
    css_width: f64,
    css_height: f64,
) -> Result<bool, String> {
    const DESIGN_W: f64 = 560.0;
    const DESIGN_H: f64 = 440.0;

    let win = app
        .get_webview_window("main")
        .ok_or("main window is unavailable")?;
    let outer = win.outer_size().map_err(|e| e.to_string())?;
    let dw = (DESIGN_W - css_width).max(0.0);
    let dh = (DESIGN_H - css_height).max(0.0);
    if dw <= 2.0 && dh <= 2.0 {
        return Ok(false);
    }
    // Guard against a runaway loop on platforms that refuse to grow the view.
    if outer.width >= 1600 || outer.height >= 1200 {
        return Ok(false);
    }

    let target = PhysicalSize::new(
        (outer.width as f64 + dw).round() as u32,
        (outer.height as f64 + dh).round() as u32,
    );
    if let Ok(pos) = win.outer_position() {
        // The stage centres the pill horizontally but pins it to the top of the
        // window, so shift horizontally and grow downwards.
        let dx = (outer.width as f64 - target.width as f64) / 2.0;
        let _ = win.set_position(PhysicalPosition::new(
            pos.x + dx.round() as i32,
            pos.y,
        ));
    }
    win.set_size(target).map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
pub fn cursor_over_island(app: AppHandle) -> Option<bool> {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::HWND;
        let win = app.get_webview_window("main")?;
        let raw = win.hwnd().ok()?;
        hit_test::cursor_inside(HWND(raw.0 as *mut core::ffi::c_void))
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        None
    }
}

/// Nudges the window so an expanded panel of the given CSS size stays fully
/// inside the monitor work area. Returns true when the window moved.
#[tauri::command]
pub fn ensure_island_visible(app: AppHandle, width: f64, height: f64) -> Result<bool, String> {
    let win = app
        .get_webview_window("main")
        .ok_or("main window is unavailable")?;
    let Some((left, top, right, bottom, scale)) = work_area(&win) else {
        return Ok(false);
    };

    let pos = win.outer_position().map_err(|e| e.to_string())?;
    let size = win.outer_size().map_err(|e| e.to_string())?;

    let pad = 6.0 * scale;
    let island_w = width * scale;
    let island_h = height * scale;
    let center = pos.x as f64 + size.width as f64 / 2.0;
    let island_y = pos.y as f64 + 5.0 * scale;

    let left = left as f64 + pad;
    let top = top as f64 + pad;
    let right = right as f64 - pad;
    let bottom = bottom as f64 - pad;

    let island_x = if island_w <= right - left {
        (center - island_w / 2.0).clamp(left, right - island_w)
    } else {
        left
    };
    let island_y = if island_h <= bottom - top {
        island_y.clamp(top, bottom - island_h)
    } else {
        top
    };

    let dx = (island_x - (center - island_w / 2.0)).round() as i32;
    let dy = (island_y - (pos.y as f64 + 5.0 * scale)).round() as i32;
    if dx == 0 && dy == 0 {
        return Ok(false);
    }

    win.set_position(PhysicalPosition::new(pos.x + dx, pos.y + dy))
        .map_err(|e| e.to_string())?;
    Ok(true)
}

/// Starts a native window move loop. Called from the pill (drag threshold) and
/// from the panel grip; using Win32 directly avoids WebView drag quirks.
#[cfg(windows)]
#[tauri::command]
pub fn start_window_drag(app: AppHandle) -> Result<(), String> {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
    use windows::Win32::UI::WindowsAndMessaging::{
        SendMessageW, HTCAPTION, WM_NCLBUTTONDOWN,
    };

    let win = app
        .get_webview_window("main")
        .ok_or("main window is unavailable")?;
    let raw = win.hwnd().map_err(|e| e.to_string())?;
    let hwnd = HWND(raw.0 as *mut core::ffi::c_void);

    unsafe {
        let _ = ReleaseCapture();
        SendMessageW(
            hwnd,
            WM_NCLBUTTONDOWN,
            Some(WPARAM(HTCAPTION as usize)),
            Some(LPARAM(0)),
        );
    }
    Ok(())
}

#[cfg(not(windows))]
#[tauri::command]
pub fn start_window_drag(_app: AppHandle) -> Result<(), String> {
    Ok(())
}

#[tauri::command]
pub fn set_window_pos(app: AppHandle, x: i32, y: i32) -> Result<(), String> {
    let win = app
        .get_webview_window("main")
        .ok_or("main window is unavailable")?;
    let (x, y) = clamp_to_monitor(&win, x, y);
    win.set_position(PhysicalPosition::new(x, y))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn reset_window_position(app: AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        place_top_center(&win);
    }
}
