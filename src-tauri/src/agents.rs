//! Live AI agent tracking.
//!
//! A tiny localhost HTTP endpoint receives normalized events from agent hooks
//! (Claude Code, ZCode, Codex, or any custom tool). Sessions are reduced to a
//! small state machine and pushed to the UI as a snapshot.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use axum::{
    extract::{Query, State as AxumState},
    http::{header::CONTENT_TYPE, HeaderValue, Method},
    response::Html,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use tower_http::cors::{AllowOrigin, CorsLayer};
use uuid::Uuid;

pub const PORT: u16 = 17321;
const SWEEP_SECS: u64 = 5;
/// Long builds legitimately run for a while; only give up on a silent session
/// after this long.
const IDLE_AFTER_MS: i64 = 15 * 60_000;
const IDLE_KEEP_MS: i64 = 5 * 60 * 1000;
const KEEP_DONE_MS: i64 = 15 * 60 * 1000;

#[derive(Clone)]
pub struct AgentHandle {
    reg: Arc<Mutex<HashMap<String, Session>>>,
    /// Permission requests the UI can answer; the HTTP request waits on one.
    pending: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<String>>>>,
    app: AppHandle,
}

struct Session {
    id: String,
    agent: String,
    title: String,
    status: String,
    task: String,
    tool: String,
    detail: String,
    progress: Option<f32>,
    cwd: String,
    model: String,
    started_at: i64,
    last_at: i64,
    completed_at: Option<i64>,
    step: u32,
    last_message: String,
    /// Set while a hook is blocked waiting for the user's decision.
    request_id: String,
}

impl Session {
    fn new(id: String, agent: String, now: i64) -> Self {
        Self {
            id,
            title: String::new(),
            agent,
            status: "idle".into(),
            task: String::new(),
            tool: String::new(),
            detail: String::new(),
            progress: None,
            cwd: String::new(),
            model: String::new(),
            started_at: now,
            last_at: now,
            completed_at: None,
            step: 0,
            last_message: String::new(),
            request_id: String::new(),
        }
    }

    fn view(&self) -> AgentView {
        AgentView {
            id: self.id.clone(),
            agent: self.agent.clone(),
            title: self.title.clone(),
            status: self.status.clone(),
            task: self.task.clone(),
            tool: self.tool.clone(),
            detail: self.detail.clone(),
            progress: self.progress,
            cwd: self.cwd.clone(),
            model: self.model.clone(),
            started_at: self.started_at,
            last_at: self.last_at,
            completed_at: self.completed_at,
            step: self.step,
            last_message: self.last_message.clone(),
            request_id: self.request_id.clone(),
        }
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AgentView {
    pub id: String,
    pub agent: String,
    pub title: String,
    pub status: String,
    pub task: String,
    pub tool: String,
    pub detail: String,
    pub progress: Option<f32>,
    pub cwd: String,
    pub model: String,
    pub started_at: i64,
    pub last_at: i64,
    pub completed_at: Option<i64>,
    pub step: u32,
    pub last_message: String,
    /// Non-empty while the agent is blocked on a permission we can answer.
    pub request_id: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct AgentEvent {
    #[serde(alias = "source")]
    agent: Option<String>,
    #[serde(alias = "hook_event_name", alias = "hookEventName", alias = "type")]
    event: Option<String>,
    #[serde(alias = "sessionId", alias = "thread_id", alias = "thread-id")]
    session_id: Option<String>,
    title: Option<String>,
    #[serde(alias = "prompt")]
    task: Option<String>,
    #[serde(alias = "tool_name", alias = "toolName")]
    tool: Option<String>,
    /// Free-form: hooks may send a string or the raw tool input object.
    detail: Option<Value>,
    #[serde(alias = "tool_input", alias = "toolInput")]
    tool_input: Option<Value>,
    status: Option<String>,
    progress: Option<f32>,
    cwd: Option<String>,
    model: Option<String>,
    #[serde(
        alias = "last-assistant-message",
        alias = "lastAssistantMessage",
        alias = "message"
    )]
    message: Option<String>,
    #[serde(alias = "requestId", alias = "request-id")]
    request_id: Option<String>,
}

pub fn start(app: AppHandle) -> Result<(), String> {
    let handle = AgentHandle {
        reg: Arc::new(Mutex::new(HashMap::new())),
        pending: Arc::new(Mutex::new(HashMap::new())),
        app: app.clone(),
    };
    app.manage(handle.clone());

    let http_handle = handle.clone();
    tauri::async_runtime::spawn(async move {
        let cors = CorsLayer::new()
            .allow_origin(AllowOrigin::list([
                HeaderValue::from_static("tauri://localhost"),
                HeaderValue::from_static("http://tauri.localhost"),
                HeaderValue::from_static("http://localhost:1420"),
            ]))
            .allow_methods([Method::GET, Method::POST])
            .allow_headers([CONTENT_TYPE]);

        let router = Router::new()
            .route("/v1/health", get(|| async { "ok" }))
            .route("/v1/agent/event", post(ingest))
            .route("/oauth/google/callback", get(google_callback))
            .with_state(http_handle.clone())
            .layer(cors);

        match tokio::net::TcpListener::bind(("127.0.0.1", PORT)).await {
            Ok(listener) => {
                let _ = axum::serve(listener, router).await;
            }
            Err(err) => {
                let _ = http_handle
                    .app
                    .emit("agents://error", format!("port {PORT}: {err}"));
            }
        }
    });

    let sweep_handle = handle.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(SWEEP_SECS)).await;
            if sweep(&sweep_handle) {
                emit(&sweep_handle);
            }
        }
    });

    Ok(())
}

async fn ingest(
    AxumState(handle): AxumState<AgentHandle>,
    Json(event): Json<AgentEvent>,
) -> Json<Value> {
    let kind = normalize_event(event.event.as_deref().unwrap_or(""));
    let request_id = event
        .request_id
        .clone()
        .filter(|value| !value.trim().is_empty());

    apply(&handle, event);

    // A permission request blocks the agent's hook until the user answers here,
    // or until the wait expires and the agent falls back to its own prompt.
    let Some(request_id) = request_id.filter(|_| kind == "waiting") else {
        return Json(json!({ "ok": true }));
    };

    let (tx, rx) = tokio::sync::oneshot::channel::<String>();
    if let Ok(mut pending) = handle.pending.lock() {
        pending.insert(request_id.clone(), tx);
    }
    let decision = match tokio::time::timeout(std::time::Duration::from_secs(75), rx).await {
        Ok(Ok(value)) => value,
        _ => {
            if let Ok(mut pending) = handle.pending.lock() {
                pending.remove(&request_id);
            }
            // Nobody answered: stop showing buttons the UI can no longer use.
            expire_request(&handle, &request_id);
            String::new()
        }
    };

    Json(json!({ "ok": true, "decision": decision }))
}

/// Clears a request that was never answered (timeout, app restart, agent gone).
fn expire_request(handle: &AgentHandle, request_id: &str) {
    let mut touched = false;
    if let Ok(mut reg) = handle.reg.lock() {
        for session in reg.values_mut() {
            if session.request_id == request_id {
                session.request_id.clear();
                if session.status == "waiting" {
                    session.status = "idle".into();
                }
                touched = true;
            }
        }
    }
    if touched {
        emit(handle);
    }
}

/// Browser redirect for the Google OAuth flow. Returns a small page the user
/// can close; the island picks the tokens up from its own state.
async fn google_callback(
    AxumState(handle): AxumState<AgentHandle>,
    Query(params): Query<HashMap<String, String>>,
) -> Html<String> {
    let message = crate::google::complete(&handle.app, &params);
    Html(format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <title>Dynamic Island</title><style>\
         html,body{{height:100%;margin:0;background:#0b0c0e;color:#f2f3f5;\
         font-family:Segoe UI,system-ui,sans-serif;display:grid;place-items:center}}\
         .card{{max-width:420px;padding:26px 30px;border-radius:18px;\
         background:#15171b;border:1px solid #24272d;text-align:center}}\
         h1{{margin:0 0 10px;font-size:17px;font-weight:600}}\
         p{{margin:0;font-size:13px;line-height:1.55;color:#a6abb5}}\
         </style></head><body><div class=\"card\"><h1>Dynamic Island</h1>\
         <p>{}</p></div></body></html>",
        escape_html(&message)
    ))
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[tauri::command]
pub fn resolve_permission(
    handle: tauri::State<'_, AgentHandle>,
    request_id: String,
    decision: String,
) -> Result<(), String> {
    let answer = match decision.to_ascii_lowercase().as_str() {
        "allow" | "allow_once" => "allow",
        "allow_always" => "allow_always",
        "allow_tool" => "allow_tool",
        _ => "deny",
    };
    let allow = answer != "deny";

    if let Ok(mut pending) = handle.pending.lock() {
        if let Some(tx) = pending.remove(&request_id) {
            let _ = tx.send(answer.to_string());
        }
    }

    let mut touched = false;
    if let Ok(mut reg) = handle.reg.lock() {
        for session in reg.values_mut() {
            if session.request_id == request_id {
                session.request_id.clear();
                session.status = if allow { "working".into() } else { "idle".into() };
                if !allow {
                    session.last_message = "Permission denied".into();
                }
                session.last_at = now_ms();
                touched = true;
            }
        }
    }
    if touched {
        emit(handle.inner());
    }
    Ok(())
}

fn apply(handle: &AgentHandle, event: AgentEvent) {
    let now = now_ms();
    let agent = normalize_agent(event.agent.as_deref().unwrap_or("generic"));
    let kind = normalize_event(event.event.as_deref().unwrap_or(""));
    let id = event
        .session_id
        .as_ref()
        .filter(|s| !s.trim().is_empty())
        .cloned()
        .unwrap_or_else(|| format!("{agent}:default"));

    let mut guard = match handle.reg.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    // A Stop/PostToolUse for a session we never saw (app restarted mid-turn, or
    // the island was closed) must not create a phantom "done" card.
    if !guard.contains_key(&id)
        && !matches!(
            kind.as_str(),
            "start" | "prompt" | "tool_start" | "waiting"
        )
    {
        return;
    }
    let session = guard
        .entry(id.clone())
        .or_insert_with(|| Session::new(id.clone(), agent.clone(), now));
    session.agent = agent.clone();
    session.last_at = now;

    if let Some(title) = event.title.as_ref().map(|s| clean(s, 64)).filter(|s| !s.is_empty()) {
        session.title = title;
    }
    if let Some(cwd) = event.cwd.as_ref().filter(|s| !s.trim().is_empty()) {
        session.cwd = cwd.clone();
        if session.title.is_empty() {
            session.title = project_title(cwd);
        }
    }
    if let Some(model) = event.model.as_ref().and_then(|s| clean_opt(s, 40)) {
        session.model = model;
    }
    if let Some(message) = event.message.as_ref().and_then(|s| clean_opt(s, 220)) {
        session.last_message = message;
    }

    match kind.as_str() {
        "start" => {
            // A session starting is not a running command; the pill only
            // lights up once a tool actually runs.
            session.status = "idle".into();
            session.started_at = now;
            session.step = 0;
            session.progress = None;
            session.completed_at = None;
            if let Some(task) = event.task.as_ref().and_then(|s| clean_opt(s, 140)) {
                session.task = task;
            }
        }
        "prompt" => {
            // The turn is alive; keep the card visible while the model thinks.
            session.status = "thinking".into();
            session.completed_at = None;
            session.progress = None;
            session.step += 1;
            if let Some(task) = event.task.as_ref().and_then(|s| clean_opt(s, 140)) {
                session.task = task;
            }
        }
        "tool_start" => {
            session.status = "working".into();
            session.step += 1;
            if let Some(tool) = event.tool.as_ref().and_then(|s| clean_opt(s, 48)) {
                session.tool = tool;
            }
            if let Some(detail) = event
                .detail
                .as_ref()
                .and_then(summarize_input)
                .or_else(|| event.tool_input.as_ref().and_then(summarize_input))
            {
                session.detail = detail;
            }
        }
        "tool_end" => {
            // The command finished but the turn continues; stay visible in a
            // calm "thinking" state instead of disappearing until the next tool.
            if session.status != "waiting" {
                session.status = "thinking".into();
            }
            if let Some(detail) = event
                .detail
                .as_ref()
                .and_then(summarize_input)
                .or_else(|| event.tool_input.as_ref().and_then(summarize_input))
            {
                session.detail = detail;
            }
        }
        "tool_error" => {
            if session.status != "waiting" {
                session.status = "thinking".into();
            }
            if let Some(message) = event.message.as_ref().and_then(|s| clean_opt(s, 160)) {
                session.last_message = message;
            }
        }
        "waiting" => {
            session.status = "waiting".into();
            if let Some(message) = event.message.as_ref().and_then(|s| clean_opt(s, 160)) {
                session.last_message = message;
            }
            if let Some(tool) = event.tool.as_ref().and_then(|s| clean_opt(s, 48)) {
                session.tool = tool;
            }
            if let Some(detail) = event
                .detail
                .as_ref()
                .and_then(summarize_input)
                .or_else(|| event.tool_input.as_ref().and_then(summarize_input))
            {
                session.detail = detail;
            }
        }
        "stop" => {
            session.status = "done".into();
            session.completed_at = Some(now);
            session.progress = Some(1.0);
        }
        "error" => {
            session.status = "error".into();
            session.completed_at = Some(now);
        }
        _ => {
            // Plain updates only refresh metadata; they never claim activity.
        }
    }

    if kind == "waiting" {
        if let Some(request) = event
            .request_id
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            session.request_id = request.clone();
        }
    } else {
        session.request_id.clear();
    }

    if let Some(status) = event.status.as_deref() {
        let s = status.to_ascii_lowercase();
        if matches!(s.as_str(), "working" | "waiting" | "thinking" | "done" | "error" | "idle") {
            session.status = s;
            if session.status == "done" && session.completed_at.is_none() {
                session.completed_at = Some(now);
                session.progress = Some(1.0);
            }
        }
    }

    if let Some(progress) = event.progress {
        let value = if progress > 1.0 { progress / 100.0 } else { progress };
        session.progress = Some(value.clamp(0.0, 1.0));
    }

    if session.title.is_empty() {
        session.title = display_agent(&session.agent);
    }

    drop(guard);
    emit(handle);
}

fn sweep(handle: &AgentHandle) -> bool {
    let now = now_ms();
    let mut changed = false;
    if let Ok(mut reg) = handle.reg.lock() {
        let before = reg.len();
        reg.retain(|_, s| match s.status.as_str() {
            "done" | "error" => s
                .completed_at
                .map(|t| now - t < KEEP_DONE_MS)
                .unwrap_or(true),
            // Sessions that went quiet are dropped after a few minutes so the
            // panel does not accumulate ghosts of long-finished commands.
            "idle" => now - s.last_at < IDLE_KEEP_MS,
            _ => true,
        });
        changed |= reg.len() != before;

        for session in reg.values_mut() {
            if matches!(session.status.as_str(), "working" | "waiting" | "thinking")
                && now - session.last_at > IDLE_AFTER_MS
            {
                session.status = "idle".into();
                changed = true;
            }
        }
    }
    changed
}

fn snapshot(handle: &AgentHandle) -> Vec<AgentView> {
    let mut views: Vec<AgentView> = match handle.reg.lock() {
        Ok(reg) => reg.values().map(Session::view).collect(),
        Err(_) => Vec::new(),
    };
    views.sort_by(|a, b| {
        rank(&a.status)
            .cmp(&rank(&b.status))
            .then_with(|| b.last_at.cmp(&a.last_at))
    });
    views.truncate(24);
    views
}

fn emit(handle: &AgentHandle) {
    let _ = handle.app.emit("agents://update", snapshot(handle));
}

fn rank(status: &str) -> u8 {
    match status {
        "waiting" => 0,
        "working" => 1,
        "thinking" => 2,
        "error" => 3,
        "idle" => 4,
        _ => 5,
    }
}

#[tauri::command]
pub fn agents_snapshot(handle: tauri::State<'_, AgentHandle>) -> Vec<AgentView> {
    snapshot(handle.inner())
}

#[tauri::command]
pub fn clear_agents(handle: tauri::State<'_, AgentHandle>) {
    if let Ok(mut reg) = handle.reg.lock() {
        reg.clear();
    }
    if let Ok(mut pending) = handle.pending.lock() {
        pending.clear();
    }
    emit(handle.inner());
}

/// Fills the island with a scripted demo so the UI can be explored without
/// configuring any hooks. Harmless: sessions are local-only.
#[tauri::command]
pub fn run_demo(handle: tauri::State<'_, AgentHandle>) -> Result<(), String> {
    let h = handle.inner().clone();
    tauri::async_runtime::spawn(async move {
        let tick = || async { tokio::time::sleep(std::time::Duration::from_millis(1300)).await };

        demo(
            &h,
            "zcode",
            "start",
            "demo:zcode",
            "dynamic-island-windows",
            "Build the island shell and design system",
            "",
            "",
        );
        demo(
            &h,
            "claude",
            "start",
            "demo:claude",
            "api-gateway",
            "",
            "",
            "",
        );
        demo(&h, "codex", "start", "demo:codex", "web-dashboard", "", "", "");
        tick().await;

        demo(
            &h,
            "zcode",
            "tool_start",
            "demo:zcode",
            "",
            "",
            "Edit",
            "src/components/Island.tsx",
        );
        demo(
            &h,
            "claude",
            "prompt",
            "demo:claude",
            "",
            "Refactor the websocket reconnection logic",
            "",
            "",
        );
        demo(
            &h,
            "claude",
            "tool_start",
            "demo:claude",
            "",
            "",
            "Read",
            "src/ws/client.ts",
        );
        demo(&h, "codex", "prompt", "demo:codex", "", "Fix flaky integration tests", "", "");
        tick().await;

        demo(
            &h,
            "codex",
            "tool_start",
            "demo:codex",
            "",
            "",
            "Bash",
            "pnpm test --filter=web",
        );
        demo(
            &h,
            "zcode",
            "tool_start",
            "demo:zcode",
            "",
            "",
            "Bash",
            "cargo check",
        );
        tick().await;

        demo(
            &h,
            "codex",
            "stop",
            "demo:codex",
            "",
            "",
            "",
            "",
        );
        demo(
            &h,
            "claude",
            "waiting",
            "demo:claude",
            "",
            "",
            "",
            "Permission required: apply database migration",
        );
        tokio::time::sleep(std::time::Duration::from_millis(2200)).await;

        demo(
            &h,
            "zcode",
            "tool_start",
            "demo:zcode",
            "",
            "",
            "Write",
            "src-tauri/src/media.rs",
        );
        tokio::time::sleep(std::time::Duration::from_millis(2600)).await;

        demo(
            &h,
            "zcode",
            "stop",
            "demo:zcode",
            "",
            "",
            "",
            "Media integration complete",
        );
        demo(
            &h,
            "claude",
            "tool_start",
            "demo:claude",
            "",
            "",
            "Bash",
            "psql -f migrations/001_add_index.sql",
        );
    });
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn demo(
    handle: &AgentHandle,
    agent: &str,
    event: &str,
    id: &str,
    title: &str,
    task: &str,
    tool: &str,
    detail: &str,
) {
    apply(
        handle,
        AgentEvent {
            agent: Some(agent.into()),
            event: Some(event.into()),
            session_id: Some(id.into()),
            title: (!title.is_empty()).then(|| title.into()),
            task: (!task.is_empty()).then(|| task.into()),
            tool: (!tool.is_empty()).then(|| tool.into()),
            detail: (!detail.is_empty()).then(|| detail.into()),
            message: if event == "stop" || event == "waiting" {
                (!detail.is_empty()).then(|| detail.into())
            } else {
                None
            },
            ..Default::default()
        },
    );
}

fn normalize_agent(raw: &str) -> String {
    let value = raw.to_ascii_lowercase();
    if value.contains("claude") {
        "claude".into()
    } else if value.contains("zcode") {
        "zcode".into()
    } else if value.contains("codex") {
        "codex".into()
    } else if value.contains("antigravity") {
        "antigravity".into()
    } else if value.contains("chatgpt") || value.contains("openai") {
        "chatgpt".into()
    } else if value.contains("gemini") {
        "gemini".into()
    } else {
        "generic".into()
    }
}

fn normalize_event(raw: &str) -> String {
    let e: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    match e.as_str() {
        "sessionstart" | "start" | "taskstarted" | "turnstarted" | "runstarted" => "start",
        "userpromptsubmit" | "prompt" | "message" | "usermessage" | "input" => "prompt",
        "pretooluse" | "toolstart" | "toolcall" | "exec" | "execcommand" => "tool_start",
        "posttooluse" | "toolend" | "toolresult" | "execend" => "tool_end",
        "posttoolusefailure" | "toolerror" | "toolfailed" | "execerror" => "tool_error",
        "permissionrequest" | "notification" | "waiting" | "approval" | "needsinput" => "waiting",
        "stop" | "agentturncomplete" | "sessionend" | "done" | "complete" | "finished" => "stop",
        "error" | "failed" | "failure" => "error",
        _ => "update",
    }
    .to_string()
}

fn display_agent(agent: &str) -> String {
    match agent {
        "claude" => "Claude Code".into(),
        "zcode" => "ZCode".into(),
        "codex" => "Codex".into(),
        "antigravity" => "Antigravity".into(),
        "chatgpt" => "ChatGPT".into(),
        "gemini" => "Gemini".into(),
        other => {
            let mut chars = other.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => "Agent".into(),
            }
        }
    }
}

fn summarize_input(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return clean_opt(text, 120);
    }
    let object = value.as_object()?;
    for key in [
        "command",
        "file_path",
        "path",
        "pattern",
        "url",
        "query",
        "description",
        "prompt",
    ] {
        if let Some(inner) = object.get(key).and_then(|v| v.as_str()) {
            if let Some(cleaned) = clean_opt(inner, 120) {
                return Some(cleaned);
            }
        }
    }
    None
}

fn clean(value: &str, max: usize) -> String {
    let flat = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        flat
    } else {
        let mut out: String = flat.chars().take(max).collect();
        out.push('…');
        out
    }
}

fn clean_opt(value: &str, max: usize) -> Option<String> {
    let cleaned = clean(value, max);
    (!cleaned.is_empty()).then_some(cleaned)
}

fn basename(path: &str) -> String {
    path.trim_end_matches(['\\', '/'])
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(path)
        .to_string()
}

/// Names a session after the nearest git repository root, so a tool that runs
/// inside `src-tauri` still shows the project name instead of a subfolder.
fn project_title(cwd: &str) -> String {
    let path = Path::new(cwd);
    let mut current = Some(path);
    for _ in 0..8 {
        let Some(dir) = current else { break };
        if dir.join(".git").exists() {
            if let Some(name) = dir.file_name().and_then(|name| name.to_str()) {
                if !name.is_empty() {
                    return name.to_string();
                }
            }
            break;
        }
        current = dir.parent();
    }
    basename(cwd)
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[allow(dead_code)]
fn _new_id() -> String {
    Uuid::new_v4().to_string()
}
