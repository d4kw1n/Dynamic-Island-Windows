use serde::Serialize;
use serde_json::{json, Value};
use std::fs;
use std::io::Read;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::agents::PORT;

/// Returns the agent name when the process was launched as a hook shim.
pub fn hook_mode_from_args() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--hook" {
            return args.get(i + 1).cloned();
        }
        if let Some(value) = args[i].strip_prefix("--hook=") {
            return Some(value.to_string());
        }
        i += 1;
    }
    None
}

/// Hook shim: normalize an agent payload, POST it to the island, exit 0.
/// It must never write to stdout (ZCode parses hook stdout strictly) and must
/// never block the agent for long.
pub fn run_hook(agent: &str) {
    let args: Vec<String> = std::env::args().collect();

    // ZCode and Claude Code write the JSON payload to stdin; a JSON argument
    // is also accepted for hand-made integrations.
    let mut payload: Option<Value> = None;
    for arg in args.iter().skip(1) {
        let trimmed = arg.trim();
        if trimmed.starts_with('{') {
            if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
                payload = Some(value);
            }
        }
    }

    if payload.is_none() {
        let mut raw = String::new();
        if std::io::stdin().read_to_string(&mut raw).is_ok() {
            if let Ok(value) = serde_json::from_str::<Value>(raw.trim()) {
                payload = Some(value);
            }
        }
    }

    let payload = payload.unwrap_or_else(|| json!({}));
    let event = first_str(
        &payload,
        &[
            "hook_event_name",
            "hookEventName",
            "event",
            "type",
        ],
    )
    .unwrap_or_else(|| "update".to_string());

    let task = first_str(&payload, &["prompt", "task"]).or_else(|| {
        payload
            .get("input-messages")
            .and_then(Value::as_array)
            .and_then(|messages| messages.last())
            .and_then(Value::as_str)
            .map(|s| s.to_string())
    });

    let message = first_str(
        &payload,
        &[
            "last-assistant-message",
            "last_assistant_message",
            "lastAssistantMessage",
            "message",
            "error",
            "errorMessage",
            "error_message",
        ],
    );

    let permission = matches!(agent, "claude" | "zcode") && is_permission_event(&event);
    let request_id = permission.then(|| uuid::Uuid::new_v4().to_string());

    let body = json!({
        "agent": agent,
        "event": event,
        "title": first_str(&payload, &["title", "sessionTitle", "session_title"]),
        "session_id": first_str(&payload, &["session_id", "sessionId", "thread-id", "thread_id", "conversation_id"]),
        "cwd": first_str(&payload, &["cwd", "working_directory"]),
        "task": task,
        "tool": first_str(&payload, &["tool_name", "toolName"]),
        "detail": payload.get("tool_input").or_else(|| payload.get("toolInput")),
        "message": message,
        "model": first_str(&payload, &["model", "model_name"]),
        "request_id": request_id,
    });

    if let Some(decision) = post(&body.to_string(), permission) {
        print_decision(&decision, &payload);
    }
}

fn is_permission_event(event: &str) -> bool {
    let normalized: String = event
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    normalized == "permissionrequest"
}

/// Answers a blocked permission hook. Claude Code and ZCode both read this
/// `hookSpecificOutput` shape (ZCode validates `hookEventName`), so the island
/// buttons become the agent's permission decision. Anything else is ignored
/// and the agent falls back to its own prompt.
fn print_decision(decision: &str, payload: &Value) {
    let tool = first_str(payload, &["tool_name", "toolName"]).unwrap_or_default();
    let input = payload.get("tool_input").or_else(|| payload.get("toolInput"));

    let decision_value = match decision {
        "allow" => json!({ "behavior": "allow" }),
        // Project rule for this exact command/path, mirroring ZCode's own
        // "Always allow in this project" option.
        "allow_always" if !tool.is_empty() => {
            let mut rule = json!({ "toolName": tool });
            if let Some(content) = input.and_then(rule_content) {
                rule["ruleContent"] = json!(content);
            }
            let updates = json!([{
                "type": "addRules",
                "behavior": "allow",
                "rules": [rule]
            }]);
            json!({
                "behavior": "allow",
                "permissionUpdates": updates.clone(),
                "updatedPermissions": updates
            })
        }
        // Every use of this tool in the project (no ruleContent).
        "allow_tool" if !tool.is_empty() => {
            let updates = json!([{
                "type": "addRules",
                "behavior": "allow",
                "rules": [{ "toolName": tool }]
            }]);
            json!({
                "behavior": "allow",
                "permissionUpdates": updates.clone(),
                "updatedPermissions": updates
            })
        }
        "allow_always" | "allow_tool" => json!({ "behavior": "allow" }),
        "deny" => json!({
            "behavior": "deny",
            "message": "Denied from Dynamic Island"
        }),
        _ => return,
    };

    // The agent parses stdout; emit nothing but the decision object.
    println!(
        "{}",
        json!({
            "hookSpecificOutput": {
                "hookEventName": "PermissionRequest",
                "decision": decision_value
            }
        })
    );
}

fn rule_content(input: &Value) -> Option<String> {
    if let Some(text) = input.as_str() {
        let text = text.trim();
        return (!text.is_empty()).then(|| text.to_string());
    }
    let object = input.as_object()?;
    for key in ["command", "url", "file_path", "path", "pattern"] {
        if let Some(value) = object.get(key).and_then(Value::as_str) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn first_str(value: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(s) = value.get(*key).and_then(Value::as_str) {
            let s = s.trim();
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
    }
    None
}

/// Posts an event. With `wait`, the island holds the response until the user
/// answers a permission prompt; returns the decision (`allow` / `deny`) or
/// `None` when there is nothing to print (no app, timeout, no answer).
fn post(body: &str, wait: bool) -> Option<String> {
    let addr = SocketAddr::from(([127, 0, 0, 1], PORT));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(350)) else {
        return None;
    };
    let _ = stream.set_write_timeout(Some(Duration::from_millis(400)));
    let read_ms = if wait { 80_000 } else { 400 };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(read_ms)));

    let request = format!(
        "POST /v1/agent/event HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    if std::io::Write::write_all(&mut stream, request.as_bytes()).is_err() {
        return None;
    }

    if !wait {
        let mut sink = [0u8; 64];
        let _ = stream.read(&mut sink);
        return None;
    }

    let mut raw: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                raw.extend_from_slice(&chunk[..n]);
                if raw.len() > 64 * 1024 {
                    break;
                }
            }
            Err(_) => break,
        }
    }

    let text = String::from_utf8_lossy(&raw);
    let payload = text.split("\r\n\r\n").nth(1)?.trim();
    let value: Value = serde_json::from_str(payload).ok()?;
    let decision = value
        .get("decision")
        .and_then(Value::as_str)
        .filter(|decision| !decision.is_empty())
        .map(|decision| decision.to_string());
    decision
}

// ---------------------------------------------------------------------------
// Hook installers
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterStatus {
    pub id: String,
    pub installed: bool,
    /// Installed hooks point at the currently running executable.
    pub current: bool,
    pub path: String,
}

#[tauri::command]
pub fn adapter_status() -> Vec<AdapterStatus> {
    let (claude_installed, claude_path) =
        json_hook_state(&claude_settings(), &["hooks"], "claude");
    let (zcode_installed, zcode_path) =
        json_hook_state(&zcode_config(), &["hooks", "events"], "zcode");
    let (codex_installed, codex_current) = codex_hook_state();

    let claude_current = claude_path
        && permission_hook_ready(&claude_settings(), &["hooks"], "claude");
    let zcode_current = zcode_path
        && permission_hook_ready(&zcode_config(), &["hooks", "events"], "zcode");

    vec![
        AdapterStatus {
            id: "claude".into(),
            installed: claude_installed,
            current: claude_current,
            path: claude_settings().to_string_lossy().to_string(),
        },
        AdapterStatus {
            id: "zcode".into(),
            installed: zcode_installed,
            current: zcode_current,
            path: zcode_config().to_string_lossy().to_string(),
        },
        AdapterStatus {
            id: "codex".into(),
            installed: codex_installed,
            current: codex_current,
            path: codex_config().to_string_lossy().to_string(),
        },
    ]
}

#[tauri::command]
pub fn install_adapter(id: String) -> Result<String, String> {
    let exe = current_exe();
    if exe.is_empty() {
        return Err("cannot resolve the application path".into());
    }

    match id.as_str() {
        "claude" => install_claude(&exe),
        "zcode" => install_zcode(&exe),
        "codex" => install_codex(&exe),
        other => Err(format!("unknown adapter: {other}")),
    }
}

#[tauri::command]
pub fn uninstall_adapter(id: String) -> Result<String, String> {
    match id.as_str() {
        "claude" => {
            remove_json_hooks(&claude_settings(), &["hooks"], "claude")?;
            Ok(format!(
                "Claude Code hooks removed from {}",
                claude_settings().display()
            ))
        }
        "zcode" => {
            remove_json_hooks(&zcode_config(), &["hooks", "events"], "zcode")?;
            Ok(format!(
                "ZCode hooks removed from {}",
                zcode_config().display()
            ))
        }
        "codex" => {
            uninstall_codex()?;
            Ok(format!(
                "Codex notify removed from {}",
                codex_config().display()
            ))
        }
        other => Err(format!("unknown adapter: {other}")),
    }
}

fn current_exe() -> String {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// True when a hook entry belongs to the island (`--hook <id>`).
fn is_island_entry(value: &Value, id: &str) -> bool {
    serde_json::to_string(value)
        .map(|text| text.contains("--hook") && text.contains(id))
        .unwrap_or(false)
}

fn normalize_path(value: &str) -> String {
    value.replace('/', "\\").to_ascii_lowercase()
}

fn value_has_exe(value: &Value, exe: &str) -> bool {
    let wanted = normalize_path(exe);
    match value {
        Value::String(text) => normalize_path(text).contains(&wanted),
        Value::Array(items) => items.iter().any(|item| value_has_exe(item, exe)),
        Value::Object(map) => map.values().any(|item| value_has_exe(item, exe)),
        _ => false,
    }
}

/// `(installed, points-at-current-exe)` for a JSON hook map.
fn json_hook_state(path: &Path, container: &[&str], id: &str) -> (bool, bool) {
    let Some(root) = fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
    else {
        return (false, false);
    };

    let mut node = &root;
    for key in container {
        match node.get(*key) {
            Some(next) => node = next,
            None => return (false, false),
        }
    }

    let exe = current_exe();
    let mut installed = false;
    let mut current = false;
    if let Some(map) = node.as_object() {
        for list in map.values() {
            let Some(items) = list.as_array() else {
                continue;
            };
            for entry in items {
                if is_island_entry(entry, id) {
                    installed = true;
                    if !exe.is_empty() && value_has_exe(entry, &exe) {
                        current = true;
                    }
                }
            }
        }
    }
    (installed, current)
}

/// The permission hook must be able to wait for an island answer. Installs from
/// before the Allow/Deny feature used 2.5–5 s timeouts, so treat those as stale
/// and let the Settings row offer Reconnect.
fn permission_hook_ready(path: &Path, container: &[&str], id: &str) -> bool {
    let Some(root) = fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
    else {
        return false;
    };
    let mut node = &root;
    for key in container {
        match node.get(*key) {
            Some(next) => node = next,
            None => return false,
        }
    }
    let Some(items) = node
        .get("PermissionRequest")
        .and_then(Value::as_array)
    else {
        return false;
    };
    items
        .iter()
        .filter(|entry| is_island_entry(entry, id))
        .any(has_long_timeout)
}

fn has_long_timeout(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(key, inner)| match key.as_str() {
            // Claude Code counts seconds, ZCode counts milliseconds.
            "timeout" => matches!(inner.as_i64(), Some(seconds) if seconds >= 60),
            "timeoutMs" => matches!(inner.as_i64(), Some(ms) if ms >= 60_000),
            _ => has_long_timeout(inner),
        }),
        Value::Array(items) => items.iter().any(has_long_timeout),
        _ => false,
    }
}

fn codex_hook_state() -> (bool, bool) {
    let Ok(text) = fs::read_to_string(codex_config()) else {
        return (false, false);
    };
    let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else {
        return (false, false);
    };
    let Some(array) = doc.get("notify").and_then(|item| item.as_array()) else {
        return (false, false);
    };

    let values: Vec<&str> = array.iter().filter_map(|value| value.as_str()).collect();
    let installed = values.contains(&"--hook") && values.contains(&"codex");
    let exe = current_exe();
    let current =
        installed && !exe.is_empty() && values.iter().any(|v| value_has_exe_str(v, &exe));
    (installed, current)
}

fn value_has_exe_str(value: &str, exe: &str) -> bool {
    normalize_path(value).contains(&normalize_path(exe))
}

fn remove_json_hooks(path: &Path, container: &[&str], id: &str) -> Result<(), String> {
    let Ok(text) = fs::read_to_string(path) else {
        return Ok(());
    };
    let mut root: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;

    let mut changed = false;
    {
        let mut node = &mut root;
        for key in container {
            match node.get_mut(*key) {
                Some(next) => node = next,
                None => return Ok(()),
            }
        }
        if let Some(map) = node.as_object_mut() {
            for list in map.values_mut() {
                if let Some(items) = list.as_array_mut() {
                    let before = items.len();
                    items.retain(|entry| !is_island_entry(entry, id));
                    changed |= before != items.len();
                }
            }
        }
    }

    if changed {
        backup(path);
        prune_empty_containers(&mut root, container);
        write_json(path, &root)?;
    }
    Ok(())
}

/// Removes hook lists that only contained our entries and any container that
/// became empty, so uninstalling leaves the user's config tidy.
fn prune_empty_containers(root: &mut Value, container: &[&str]) {
    if let Some(node) = get_mut_path(root, container) {
        if let Some(map) = node.as_object_mut() {
            map.retain(|_, value| !matches!(value.as_array(), Some(items) if items.is_empty()));
        }
    }

    for depth in (1..=container.len()).rev() {
        let (parent_path, key) = container.split_at(depth - 1);
        let key = key[0];
        let is_empty = get_mut_path(root, parent_path)
            .and_then(|parent| parent.get(key))
            .and_then(|value| value.as_object())
            .map(|object| object.is_empty())
            .unwrap_or(false);
        if !is_empty {
            break;
        }
        if let Some(parent) = get_mut_path(root, parent_path).and_then(|p| p.as_object_mut()) {
            parent.remove(key);
        }
    }
}

fn get_mut_path<'a>(root: &'a mut Value, path: &[&str]) -> Option<&'a mut Value> {
    let mut node = root;
    for key in path {
        node = node.get_mut(*key)?;
    }
    Some(node)
}

fn uninstall_codex() -> Result<(), String> {
    let path = codex_config();
    let Ok(text) = fs::read_to_string(&path) else {
        return Ok(());
    };
    let Ok(mut doc) = text.parse::<toml_edit::DocumentMut>() else {
        return Ok(());
    };

    let is_island = doc
        .get("notify")
        .and_then(|item| item.as_array())
        .map(|array| {
            let values: Vec<&str> = array.iter().filter_map(|v| v.as_str()).collect();
            values.contains(&"--hook") && values.contains(&"codex")
        })
        .unwrap_or(false);

    if is_island {
        backup(&path);
        doc.remove("notify");
        fs::write(&path, doc.to_string()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn claude_settings() -> PathBuf {
    home().join(".claude").join("settings.json")
}

fn zcode_config() -> PathBuf {
    home().join(".zcode").join("cli").join("config.json")
}

fn codex_config() -> PathBuf {
    home().join(".codex").join("config.toml")
}

fn backup(path: &Path) {
    if !path.exists() {
        return;
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "config".into());
    let target = path.with_file_name(format!(
        "{name}.island-backup-{}",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    ));
    let _ = fs::copy(path, target);
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    fs::write(path, text + "\n").map_err(|e| e.to_string())
}

fn install_claude(exe: &str) -> Result<String, String> {
    let path = claude_settings();
    backup(&path);

    let mut root: Value = fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| json!({}));

    if !root.is_object() {
        root = json!({});
    }
    if !root.get("hooks").map(Value::is_object).unwrap_or(false) {
        root["hooks"] = json!({});
    }

    let command = format!("\"{exe}\" --hook claude");

    let hooks = root["hooks"].as_object_mut().ok_or("hooks is not an object")?;
    // Replace stale island entries (for example hooks pointing at an old
    // build) instead of leaving them behind next to the new command.
    for list in hooks.values_mut() {
        if let Some(array) = list.as_array_mut() {
            array.retain(|entry| !is_island_entry(entry, "claude"));
        }
    }
    for event in [
        "SessionStart",
        "UserPromptSubmit",
        "PreToolUse",
        "PermissionRequest",
        "PostToolUse",
        "PostToolUseFailure",
        "Notification",
        "Stop",
        "SessionEnd",
    ] {
        // The permission hook waits for the island, so give it room; the rest
        // stay short so an unreachable island can never slow the agent down.
        let timeout = if event == "PermissionRequest" { 90 } else { 5 };
        let hook_entry = json!({
            "hooks": [
                {
                    "type": "command",
                    "command": command,
                    "timeout": timeout
                }
            ]
        });
        let list = hooks
            .entry(event.to_string())
            .or_insert_with(|| json!([]));
        if let Some(array) = list.as_array_mut() {
            let already = array.iter().any(|entry| {
                serde_json::to_string(entry)
                    .map(|text| text.contains("--hook") && text.contains("claude"))
                    .unwrap_or(false)
            });
            if !already {
                array.push(hook_entry);
            }
        }
    }

    write_json(&path, &root)?;
    Ok(format!("Claude Code hooks installed in {}", path.display()))
}

fn install_zcode(exe: &str) -> Result<String, String> {
    let path = zcode_config();
    backup(&path);

    let mut root: Value = fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| json!({}));
    if !root.is_object() {
        root = json!({});
    }

    if !root.get("hooks").map(Value::is_object).unwrap_or(false) {
        root["hooks"] = json!({ "enabled": true, "events": {} });
    }
    root["hooks"]["enabled"] = json!(true);
    if !root["hooks"].get("events").map(Value::is_object).unwrap_or(false) {
        root["hooks"]["events"] = json!({});
    }

    let events = root["hooks"]["events"]
        .as_object_mut()
        .ok_or("hooks.events is not an object")?;
    for list in events.values_mut() {
        if let Some(array) = list.as_array_mut() {
            array.retain(|entry| !is_island_entry(entry, "zcode"));
        }
    }
    for event in [
        "SessionStart",
        "UserPromptSubmit",
        "PreToolUse",
        "PermissionRequest",
        "PostToolUse",
        "PostToolUseFailure",
        "Stop",
    ] {
        // The permission hook blocks until the island answers, so it needs a
        // much larger budget than the fire-and-forget ones.
        let timeout = if event == "PermissionRequest" {
            80_000
        } else {
            2500
        };
        let hook_entry = json!({
            "hooks": [
                {
                    "type": "process",
                    "command": exe,
                    "args": ["--hook", "zcode"],
                    "timeoutMs": timeout
                }
            ]
        });
        let list = events
            .entry(event.to_string())
            .or_insert_with(|| json!([]));
        if let Some(array) = list.as_array_mut() {
            let already = array.iter().any(|entry| {
                serde_json::to_string(entry)
                    .map(|text| text.contains("--hook") && text.contains("zcode"))
                    .unwrap_or(false)
            });
            if !already {
                array.push(hook_entry);
            }
        }
    }

    write_json(&path, &root)?;
    Ok(format!("ZCode hooks installed in {}", path.display()))
}

fn install_codex(exe: &str) -> Result<String, String> {
    let path = codex_config();
    backup(&path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let text = fs::read_to_string(&path).unwrap_or_default();
    let mut doc: toml_edit::DocumentMut = text.parse().unwrap_or_default();

    let mut notify = toml_edit::Array::new();
    notify.push(exe);
    notify.push("--hook");
    notify.push("codex");
    doc["notify"] = toml_edit::Item::Value(toml_edit::Value::Array(notify));

    fs::write(&path, doc.to_string()).map_err(|e| e.to_string())?;
    Ok(format!(
        "Codex notify installed in {}. Codex reports turn completion only.",
        path.display()
    ))
}
