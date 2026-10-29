#![windows_subsystem = "windows"]

mod agents;
mod app_cmds;
mod calendar;
mod fullscreen;
mod google;
mod hit_test;
mod hooks;
mod media;
mod procs;
mod store;
mod windowing;

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let path = std::env::temp_dir().join("dynamic-island-crash.log");
        let _ = std::fs::write(path, format!("{info}
"));
    }));

    // Hook shim mode: no UI, no Tauri startup. The process is spawned by
    // agent hook runners and must exit in a few milliseconds.
    if let Some(agent) = hooks::hook_mode_from_args() {
        hooks::run_hook(&agent);
        return;
    }

    // Debug-only iCalendar parser check.
    #[cfg(debug_assertions)]
    {
        let args: Vec<String> = std::env::args().collect();
        if args.len() >= 3 && args[1] == "--ics-check" {
            calendar::debug_dump(&args[2]);
            return;
        }
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            windowing::show_island(app);
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(media::MediaState::default())
        .manage(store::StoreState::default())
        .manage(calendar::CalendarState::default())
        .invoke_handler(tauri::generate_handler![
            windowing::set_hit_rect,
            windowing::cursor_over_island,
            windowing::fit_island_window,
            windowing::ensure_island_visible,
            windowing::set_window_pos,
            windowing::reset_window_position,
            windowing::start_window_drag,
            media::media_control,
            store::load_state,
            store::save_state,
            store::app_paths,
            store::open_data_dir,
            store::quit_app,
            app_cmds::set_autostart,
            app_cmds::get_autostart,
            app_cmds::notify,
            hooks::adapter_status,
            hooks::install_adapter,
            hooks::uninstall_adapter,
            agents::agents_snapshot,
            agents::clear_agents,
            agents::resolve_permission,
            agents::run_demo,
            calendar::calendar_set_source,
            calendar::calendar_refresh,
            google::google_status,
            google::google_connect,
            google::google_disconnect,
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            store::init(&handle).map_err(std::io::Error::other)?;
            windowing::setup(app)?;
            google::init(&handle);
            media::start(handle.clone());
            agents::start(handle.clone()).map_err(std::io::Error::other)?;
            procs::start(handle.clone());
            fullscreen::start(handle.clone());
            calendar::start(handle.clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Dynamic Island");
}
