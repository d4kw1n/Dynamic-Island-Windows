//! Detects agent applications that are running, even when no hook is
//! configured. This lets the island show "Antigravity is open" for tools that
//! do not expose an event API.

use std::collections::HashMap;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};

#[derive(Serialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RunningSource {
    pub id: String,
    pub count: u32,
}

pub fn start(app: AppHandle) {
    std::thread::Builder::new()
        .name("island-procs".into())
        .spawn(move || {
            let mut last = String::new();
            loop {
                let sources = scan();
                let json = serde_json::to_string(&sources).unwrap_or_default();
                if json != last {
                    let _ = app.emit("agents://sources", &sources);
                    last = json;
                }
                std::thread::sleep(Duration::from_secs(4));
            }
        })
        .ok();
}

fn scan() -> Vec<RunningSource> {
    let mut counts: HashMap<&'static str, u32> = HashMap::new();

    unsafe {
        if let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            let mut entry = PROCESSENTRY32W::default();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

            if Process32FirstW(snapshot, &mut entry).is_ok() {
                loop {
                    let name = String::from_utf16_lossy(&entry.szExeFile);
                    let name = name.trim_end_matches('\0').to_ascii_lowercase();
                    if let Some(id) = map_exe(&name) {
                        *counts.entry(id).or_insert(0) += 1;
                    }
                    if Process32NextW(snapshot, &mut entry).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snapshot);
        }
    }

    let mut result: Vec<RunningSource> = counts
        .into_iter()
        .map(|(id, count)| RunningSource {
            id: id.to_string(),
            count,
        })
        .collect();
    result.sort_by(|a, b| a.id.cmp(&b.id));
    result
}

fn map_exe(name: &str) -> Option<&'static str> {
    match name {
        "zcode.exe" => Some("zcode"),
        "claude.exe" => Some("claude"),
        "codex.exe" => Some("codex"),
        "antigravity.exe" | "antigravity ide.exe" => Some("antigravity"),
        "chatgpt.exe" => Some("chatgpt"),
        "gemini.exe" => Some("gemini"),
        _ => None,
    }
}
