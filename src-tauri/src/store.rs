use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

use crate::agents::PORT;

#[derive(Default)]
pub struct StoreState {
    path: Mutex<Option<PathBuf>>,
}

pub fn init(app: &AppHandle) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let path = dir.join("state.json");
    if !path.exists() {
        let _ = fs::write(&path, "{}\n");
    }

    let state = app.state::<StoreState>();
    let mut guard = state.path.lock().map_err(|e| e.to_string())?;
    *guard = Some(path);
    Ok(())
}

fn data_dir(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok()
}

/// Reads the saved island position before the window is first placed.
pub fn saved_window_pos(app: &AppHandle) -> Option<(i32, i32)> {
    let path = data_dir(app)?.join("state.json");
    let text = fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    let pos = value.get("windowPos")?;
    let x = pos.get("x")?.as_i64()? as i32;
    let y = pos.get("y")?.as_i64()? as i32;
    Some((x, y))
}

#[tauri::command]
pub fn load_state(state: tauri::State<'_, StoreState>) -> Value {
    let Ok(guard) = state.path.lock() else {
        return json!({});
    };
    let Some(path) = guard.as_ref() else {
        return json!({});
    };
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or_else(|| json!({}))
}

#[tauri::command]
pub fn save_state(state: tauri::State<'_, StoreState>, data: Value) -> Result<(), String> {
    let guard = state.path.lock().map_err(|e| e.to_string())?;
    let path = guard.as_ref().ok_or("state store is not ready")?;
    let temp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec(&data).map_err(|e| e.to_string())?;
    fs::write(&temp, bytes).map_err(|e| e.to_string())?;
    fs::rename(&temp, path).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn app_paths(app: AppHandle) -> Value {
    json!({
        "version": app.package_info().version.to_string(),
        "exe": std::env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
        "dataDir": data_dir(&app)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
        "port": PORT,
    })
}

#[tauri::command]
pub fn open_data_dir(app: AppHandle) -> Result<(), String> {
    let dir = data_dir(&app).ok_or("app data directory is unavailable")?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::process::Command::new("explorer")
        .arg(dir)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}
