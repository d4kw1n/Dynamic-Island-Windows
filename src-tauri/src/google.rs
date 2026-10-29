//! Google Calendar sign-in.
//!
//! A desktop OAuth client (client ID + secret from the Google Cloud Console)
//! authorizes the app through the system browser. The redirect goes to the
//! local webhook server on `127.0.0.1:17321`, the code is exchanged with PKCE,
//! and the refresh token is kept in the app data folder. Events are read from
//! the primary calendar with the read-only scope and published through the
//! same `calendar://update` channel as iCal feeds.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use base64::Engine;
use chrono::{DateTime, Local, NaiveDate, SecondsFormat, TimeZone};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};
use url::Url;
use uuid::Uuid;

use crate::agents::PORT;
use crate::calendar::CalendarEvent;

const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
const CALENDAR_ENDPOINT: &str = "https://www.googleapis.com/calendar/v3/calendars";
const CALENDAR_LIST_ENDPOINT: &str =
    "https://www.googleapis.com/calendar/v3/users/me/calendarList";
const SCOPE: &str = "https://www.googleapis.com/auth/calendar.readonly";

#[derive(Default)]
struct GoogleInner {
    client_id: String,
    client_secret: String,
    access_token: String,
    refresh_token: String,
    expires_at: i64,
    pending: Option<PendingAuth>,
    error: Option<String>,
}

struct PendingAuth {
    state: String,
    verifier: String,
}

#[derive(Default)]
pub struct GoogleAuth {
    inner: Mutex<GoogleInner>,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct GoogleStatus {
    pub connected: bool,
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Saved {
    client_id: String,
    client_secret: String,
    access_token: String,
    refresh_token: String,
    expires_at: i64,
}

pub fn init(app: &AppHandle) {
    let mut inner = GoogleInner::default();
    if let Ok(text) = fs::read_to_string(config_path(app)) {
        if let Ok(saved) = serde_json::from_str::<Saved>(&text) {
            inner.client_id = saved.client_id;
            inner.client_secret = saved.client_secret;
            inner.access_token = saved.access_token;
            inner.refresh_token = saved.refresh_token;
            inner.expires_at = saved.expires_at;
        }
    }
    app.manage(GoogleAuth {
        inner: Mutex::new(inner),
    });
}

fn config_path(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("google.json")
}

fn save(app: &AppHandle, inner: &GoogleInner) {
    let saved = Saved {
        client_id: inner.client_id.clone(),
        client_secret: inner.client_secret.clone(),
        access_token: inner.access_token.clone(),
        refresh_token: inner.refresh_token.clone(),
        expires_at: inner.expires_at,
    };
    if let Ok(text) = serde_json::to_string_pretty(&saved) {
        let path = config_path(app);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(path, text);
    }
}

/// Redirect URI registered on the OAuth client. Loopback redirects may use any
/// port for desktop clients, but the path has to match.
pub fn redirect_uri() -> String {
    format!("http://127.0.0.1:{PORT}/oauth/google/callback")
}

fn status(app: &AppHandle) -> GoogleStatus {
    match app.state::<GoogleAuth>().inner.lock() {
        Ok(inner) => GoogleStatus {
            connected: !inner.refresh_token.is_empty(),
            error: inner.error.clone(),
        },
        Err(_) => GoogleStatus::default(),
    }
}

pub fn connected(app: &AppHandle) -> bool {
    app.state::<GoogleAuth>()
        .inner
        .lock()
        .map(|inner| !inner.refresh_token.is_empty())
        .unwrap_or(false)
}

#[tauri::command]
pub fn google_status(app: AppHandle) -> GoogleStatus {
    status(&app)
}

#[tauri::command]
pub fn google_connect(
    app: AppHandle,
    client_id: String,
    client_secret: String,
) -> Result<(), String> {
    let client_id = client_id.trim().to_string();
    if client_id.is_empty() {
        return Err("Paste the OAuth client ID from the Google Cloud Console".into());
    }

    let verifier = format!(
        "{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    );
    let challenge = b64url(&Sha256::digest(verifier.as_bytes()));
    let state = Uuid::new_v4().to_string();

    {
        let auth = app.state::<GoogleAuth>();
        let mut inner = auth.inner.lock().map_err(|_| "Google state unavailable")?;
        inner.client_id = client_id.clone();
        inner.client_secret = client_secret.trim().to_string();
        inner.pending = Some(PendingAuth {
            state: state.clone(),
            verifier,
        });
        inner.error = None;
        save(&app, &inner);
    }

    let mut url = Url::parse(AUTH_ENDPOINT).map_err(|error| error.to_string())?;
    url.query_pairs_mut()
        .append_pair("client_id", &client_id)
        .append_pair("redirect_uri", &redirect_uri())
        .append_pair("response_type", "code")
        .append_pair("scope", SCOPE)
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent")
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &state);

    open_browser(url.as_str())?;
    let _ = app.emit("google://status", status(&app));
    Ok(())
}

#[tauri::command]
pub fn google_disconnect(app: AppHandle) -> Result<(), String> {
    {
        let auth = app.state::<GoogleAuth>();
        let mut inner = auth.inner.lock().map_err(|_| "Google state unavailable")?;
        inner.access_token.clear();
        inner.refresh_token.clear();
        inner.expires_at = 0;
        inner.pending = None;
        inner.error = None;
        save(&app, &inner);
    }
    let _ = app.emit("google://status", status(&app));
    if crate::calendar::current_source(&app).is_empty() {
        crate::calendar::clear_google(&app);
    }
    Ok(())
}

/// Handles the browser redirect served by the local webhook. Returns the text
/// shown on the "you can close this tab" page.
pub fn complete(app: &AppHandle, params: &HashMap<String, String>) -> String {

    let pending = {
        let auth = app.state::<GoogleAuth>();
        let Ok(mut inner) = auth.inner.lock() else {
            return "The island could not read its Google state.".into();
        };
        inner.pending.take()
    };
    let Some(pending) = pending else {
        return "This sign-in link is no longer valid. Start again from the island.".into();
    };

    if params.get("state") != Some(&pending.state) {
        set_error(app, "Google sign-in failed: state mismatch".into());
        return "The sign-in response did not match this request. Try again.".into();
    }

    if let Some(error) = params.get("error") {
        let message = format!("Google sign-in cancelled: {error}");
        set_error(app, message.clone());
        return message;
    }

    let Some(code) = params.get("code").cloned() else {
        set_error(app, "Google sign-in returned no code".into());
        return "Google did not return an authorization code.".into();
    };

    let (client_id, client_secret) = {
        let auth = app.state::<GoogleAuth>();
        let guard = auth.inner.lock();
        match guard {
            Ok(inner) => (inner.client_id.clone(), inner.client_secret.clone()),
            Err(_) => return "The island could not read its Google state.".into(),
        }
    };

    let form = vec![
        ("client_id".to_string(), client_id),
        ("client_secret".to_string(), client_secret),
        ("code".to_string(), code),
        ("code_verifier".to_string(), pending.verifier),
        ("grant_type".to_string(), "authorization_code".to_string()),
        ("redirect_uri".to_string(), redirect_uri()),
    ];

    match exchange(form) {
        Ok(value) => {
            let auth = app.state::<GoogleAuth>();
            let applied = auth.inner.lock().map(|mut inner| {
                let result = apply_token(&mut inner, &value);
                if result.is_ok() {
                    inner.error = None;
                }
                save(&app, &inner);
                result
            });
            match applied {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    set_error(app, error.clone());
                    return error;
                }
                Err(_) => return "The island could not store the Google tokens.".into(),
            }

            let _ = app.emit("google://status", status(app));
            crate::calendar::sync_google(app);
            "Google Calendar connected. You can close this tab and return to the island."
                .into()
        }
        Err(error) => {
            set_error(app, error.clone());
            error
        }
    }
}

fn set_error(app: &AppHandle, message: String) {
    {
        let auth = app.state::<GoogleAuth>();
        let guard = auth.inner.lock();
        if let Ok(mut inner) = guard {
            inner.error = Some(message);
        }
    }
    let _ = app.emit("google://status", status(app));
}

fn apply_token(inner: &mut GoogleInner, value: &Value) -> Result<(), String> {
    let access = value
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or("Google returned no access token")?;
    inner.access_token = access.to_string();
    if let Some(refresh) = value.get("refresh_token").and_then(Value::as_str) {
        if !refresh.is_empty() {
            inner.refresh_token = refresh.to_string();
        }
    }
    let expires = value
        .get("expires_in")
        .and_then(Value::as_i64)
        .unwrap_or(3600);
    inner.expires_at = chrono::Utc::now().timestamp_millis() + expires * 1000;
    Ok(())
}

/// Returns a valid access token, refreshing it when it is about to expire.
fn access_token(app: &AppHandle) -> Result<String, String> {
    let auth = app.state::<GoogleAuth>();
    let mut inner = auth.inner.lock().map_err(|_| "Google state unavailable")?;
    let now = chrono::Utc::now().timestamp_millis();

    if !inner.access_token.is_empty() && inner.expires_at > now + 30_000 {
        return Ok(inner.access_token.clone());
    }
    if inner.refresh_token.is_empty() {
        return Err("Google Calendar is not connected".into());
    }

    let form = vec![
        ("client_id".to_string(), inner.client_id.clone()),
        ("client_secret".to_string(), inner.client_secret.clone()),
        ("refresh_token".to_string(), inner.refresh_token.clone()),
        ("grant_type".to_string(), "refresh_token".to_string()),
    ];
    let value = exchange(form)?;
    apply_token(&mut inner, &value)?;
    save(app, &inner);
    Ok(inner.access_token.clone())
}

fn invalidate_access(app: &AppHandle) {
    let auth = app.state::<GoogleAuth>();
    let guard = auth.inner.lock();
    if let Ok(mut inner) = guard {
        inner.expires_at = 0;
    }
}

/// Fetches the next two weeks from every calendar the account shows in
/// Google Calendar (primary plus subscribed calendars). Users often keep their
/// events on a secondary calendar, so reading `primary` alone looks empty.
pub fn fetch_events(app: &AppHandle) -> Result<Vec<CalendarEvent>, String> {
    let now = Local::now();
    let min = (now - chrono::Duration::days(1)).to_rfc3339_opts(SecondsFormat::Secs, true);
    let max = (now + chrono::Duration::days(14)).to_rfc3339_opts(SecondsFormat::Secs, true);

    let calendars = match list_calendars(app) {
        Ok(list) if !list.is_empty() => list,
        // A restricted token may not be allowed to list calendars; fall back
        // to the primary one so the basic case still works.
        _ => vec![("primary".to_string(), "Google".to_string())],
    };

    let mut merged: Vec<CalendarEvent> = Vec::new();
    let mut first_error: Option<String> = None;
    let mut succeeded = 0usize;
    for (id, name) in &calendars {
        match fetch_calendar(app, id, name, &min, &max) {
            Ok(mut events) => {
                succeeded += 1;
                merged.append(&mut events);
            }
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
    }
    if succeeded == 0 {
        return Err(first_error.unwrap_or_else(|| "Google Calendar returned no data".into()));
    }

    merged.sort_by_key(|event| event.start);
    let mut seen = std::collections::HashSet::new();
    merged.retain(|event| {
        seen.insert(format!(
            "{}|{}|{}|{}",
            event.title, event.start, event.end, event.all_day
        ))
    });
    merged.truncate(80);
    Ok(merged)
}

/// Calendars visible in the user's Google Calendar, capped to keep the sync
/// light. Read-only calendars are included; free/busy entries are not.
fn list_calendars(app: &AppHandle) -> Result<Vec<(String, String)>, String> {
    let (status, body) = get_json(
        app,
        &format!("{CALENDAR_LIST_ENDPOINT}?maxResults=50&minAccessRole=reader"),
    )?;
    if status >= 400 {
        return Err(google_error(status, &body));
    }
    let root: Value = serde_json::from_str(&body)
        .map_err(|error| format!("Google Calendar response: {error}"))?;
    let items = root
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut calendars = Vec::new();
    for item in items {
        if item.get("deleted").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        if item.get("selected").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        if item.get("accessRole").and_then(Value::as_str) == Some("freeBusyReader") {
            continue;
        }
        let Some(id) = item.get("id").and_then(Value::as_str) else {
            continue;
        };
        let name = item
            .get("summary")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("Google");
        calendars.push((id.to_string(), name.to_string()));
        if calendars.len() >= 12 {
            break;
        }
    }
    Ok(calendars)
}

fn fetch_calendar(
    app: &AppHandle,
    calendar_id: &str,
    name: &str,
    min: &str,
    max: &str,
) -> Result<Vec<CalendarEvent>, String> {
    let mut url = Url::parse(CALENDAR_ENDPOINT).map_err(|error| error.to_string())?;
    url.path_segments_mut()
        .map_err(|_| "Invalid calendar id")?
        .push(calendar_id)
        .push("events");
    url.query_pairs_mut()
        .append_pair("timeMin", min)
        .append_pair("timeMax", max)
        .append_pair("singleEvents", "true")
        .append_pair("orderBy", "startTime")
        .append_pair("maxResults", "40");

    let (status, body) = get_json(app, url.as_str())?;
    if status >= 400 {
        return Err(google_error(status, &body));
    }
    parse_events(&body, name)
}

/// Authenticated GET with a single refresh-and-retry on 401.
fn get_json(app: &AppHandle, url: &str) -> Result<(u32, String), String> {
    let mut token = access_token(app)?;
    let mut response = http_request(
        "GET",
        url,
        &[("Authorization".into(), format!("Bearer {token}"))],
        None,
    )?;
    if response.0 == 401 {
        // Token revoked or expired early: refresh once and retry.
        invalidate_access(app);
        token = access_token(app)?;
        response = http_request(
            "GET",
            url,
            &[("Authorization".into(), format!("Bearer {token}"))],
            None,
        )?;
    }
    Ok(response)
}

/// Turns Google's JSON error into something the Settings row can show.
fn google_error(status: u32, body: &str) -> String {
    let value: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let error = value.get("error");
    let message = error
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str);
    let reason = error
        .and_then(|error| error.get("errors"))
        .and_then(Value::as_array)
        .and_then(|errors| errors.first())
        .and_then(|error| error.get("reason"))
        .and_then(Value::as_str);

    match (reason, message) {
        (Some("accessNotConfigured") | Some("SERVICE_DISABLED"), Some(message)) => format!(
            "Google Calendar API is disabled for this project. Enable it, then press Sync. {message}"
        ),
        (Some("insufficientPermissions") | Some("ACCESS_TOKEN_SCOPE_INSUFFICIENT"), _) => {
            "The Google token lacks the Calendar scope. Disconnect and connect again.".into()
        }
        (_, Some(message)) => format!("Google Calendar returned HTTP {status}: {message}"),
        _ => format!("Google Calendar returned HTTP {status}"),
    }
}

/// Called by the calendar loop when no iCal feed is configured.
pub fn sync(app: &AppHandle) {
    if !connected(app) {
        return;
    }
    let result = fetch_events(app);
    crate::calendar::publish_google(app, result);
}

fn parse_events(text: &str, calendar: &str) -> Result<Vec<CalendarEvent>, String> {
    let root: Value =
        serde_json::from_str(text).map_err(|error| format!("Google Calendar response: {error}"))?;
    let items = root
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut events = Vec::new();
    for item in items {
        if item.get("status").and_then(Value::as_str) == Some("cancelled") {
            continue;
        }
        let Some((start, all_day)) = parse_point(item.get("start")) else {
            continue;
        };
        let (end, _) = parse_point(item.get("end")).unwrap_or((start + 30 * 60_000, false));
        events.push(CalendarEvent {
            id: item
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("google")
                .to_string(),
            title: item
                .get("summary")
                .and_then(Value::as_str)
                .unwrap_or("(No title)")
                .to_string(),
            start,
            end: end.max(start + 1),
            all_day,
            location: item
                .get("location")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            calendar: calendar.to_string(),
        });
        if events.len() >= 80 {
            break;
        }
    }
    Ok(events)
}

fn parse_point(value: Option<&Value>) -> Option<(i64, bool)> {
    let value = value?;
    if let Some(datetime) = value.get("dateTime").and_then(Value::as_str) {
        let parsed = DateTime::parse_from_rfc3339(datetime).ok()?;
        return Some((parsed.timestamp_millis(), false));
    }
    if let Some(date) = value.get("date").and_then(Value::as_str) {
        let day = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
        let midnight = day.and_hms_opt(0, 0, 0)?;
        let local = Local
            .from_local_datetime(&midnight)
            .earliest()
            .or_else(|| Local.from_local_datetime(&midnight).latest())?;
        return Some((local.timestamp_millis(), true));
    }
    None
}

fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn exchange(form: Vec<(String, String)>) -> Result<Value, String> {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in &form {
        serializer.append_pair(key, value);
    }
    let body = serializer.finish();

    let (status, text) = http_request(
        "POST",
        TOKEN_ENDPOINT,
        &[(
            "Content-Type".into(),
            "application/x-www-form-urlencoded".into(),
        )],
        Some(body.as_bytes()),
    )?;

    let value: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!({}));
    if status >= 400 {
        let reason = value
            .get("error_description")
            .or_else(|| value.get("error"))
            .and_then(Value::as_str)
            .unwrap_or("token request failed");
        return Err(format!("Google sign-in failed: {reason}"));
    }
    Ok(value)
}

fn open_browser(url: &str) -> Result<(), String> {
    std::process::Command::new("rundll32.exe")
        .arg("url.dll,FileProtocolHandler")
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Cannot open the browser: {error}"))
}

// ---------------------------------------------------------------------------
// Minimal WinHTTP client (the calendar module has a GET-only copy; OAuth needs
// custom headers and a form body).
// ---------------------------------------------------------------------------

#[cfg(windows)]
struct InternetHandle(*mut core::ffi::c_void);

#[cfg(windows)]
impl Drop for InternetHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = windows::Win32::Networking::WinHttp::WinHttpCloseHandle(self.0);
            }
        }
    }
}

#[cfg(windows)]
fn http_request(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Option<&[u8]>,
) -> Result<(u32, String), String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Networking::WinHttp::{
        WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable,
        WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
        WinHttpSetOption, WinHttpSetTimeouts, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
        WINHTTP_FLAG_SECURE, WINHTTP_OPEN_REQUEST_FLAGS, WINHTTP_OPTION_REDIRECT_POLICY,
        WINHTTP_OPTION_REDIRECT_POLICY_ALWAYS, WINHTTP_QUERY_FLAG_NUMBER,
        WINHTTP_QUERY_STATUS_CODE,
    };

    let parsed = Url::parse(url).map_err(|error| format!("Invalid URL: {error}"))?;
    let secure = match parsed.scheme() {
        "https" => true,
        "http" => false,
        other => return Err(format!("Unsupported scheme: {other}")),
    };
    let host = parsed
        .host_str()
        .ok_or("Google URL has no host")?
        .to_string();
    let port = parsed
        .port_or_known_default()
        .unwrap_or(if secure { 443 } else { 80 });

    let mut object = parsed.path().to_string();
    if let Some(query) = parsed.query() {
        object.push('?');
        object.push_str(query);
    }
    if object.is_empty() {
        object.push('/');
    }

    let header_text = headers
        .iter()
        .map(|(key, value)| format!("{key}: {value}"))
        .collect::<Vec<_>>()
        .join("\r\n");
    let header_wide: Vec<u16> = header_text.encode_utf16().collect();

    unsafe {
        let agent = HSTRING::from("DynamicIsland/0.1");
        let session = WinHttpOpen(
            &agent,
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        );
        if session.is_null() {
            return Err("WinHTTP could not start".into());
        }
        let session = InternetHandle(session);
        let _ = WinHttpSetTimeouts(session.0, 8000, 8000, 12000, 20000);
        let redirect: u32 = WINHTTP_OPTION_REDIRECT_POLICY_ALWAYS;
        let _ = WinHttpSetOption(
            Some(session.0 as *const _),
            WINHTTP_OPTION_REDIRECT_POLICY,
            Some(&redirect.to_le_bytes()),
        );

        let host_w = HSTRING::from(host);
        let object_w = HSTRING::from(object);
        let verb_w = HSTRING::from(method);

        let connect = WinHttpConnect(session.0, &host_w, port as u16, 0);
        if connect.is_null() {
            return Err("Cannot reach Google".into());
        }
        let connect = InternetHandle(connect);

        let request = WinHttpOpenRequest(
            connect.0,
            &verb_w,
            &object_w,
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            if secure {
                WINHTTP_FLAG_SECURE
            } else {
                WINHTTP_OPEN_REQUEST_FLAGS(0)
            },
        );
        if request.is_null() {
            return Err("Cannot open the Google request".into());
        }
        let request = InternetHandle(request);

        let header_slice = if header_wide.is_empty() {
            None
        } else {
            Some(header_wide.as_slice())
        };
        WinHttpSendRequest(
            request.0,
            header_slice,
            body.map(|bytes| bytes.as_ptr() as *const core::ffi::c_void),
            body.map(|bytes| bytes.len() as u32).unwrap_or(0),
            body.map(|bytes| bytes.len() as u32).unwrap_or(0),
            0,
        )
        .map_err(|error| format!("Google request failed: {error}"))?;

        WinHttpReceiveResponse(request.0, std::ptr::null_mut())
            .map_err(|error| format!("Google did not answer: {error}"))?;

        let mut status: u32 = 0;
        let mut length: u32 = 4;
        let _ = WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut _),
            &mut length,
            std::ptr::null_mut(),
        );

        let mut body_bytes: Vec<u8> = Vec::new();
        loop {
            let mut available: u32 = 0;
            WinHttpQueryDataAvailable(request.0, &mut available)
                .map_err(|error| format!("Google read failed: {error}"))?;
            if available == 0 {
                break;
            }
            let chunk = available.min(64 * 1024);
            let mut buffer = vec![0u8; chunk as usize];
            let mut read: u32 = 0;
            WinHttpReadData(
                request.0,
                buffer.as_mut_ptr() as *mut _,
                chunk,
                &mut read,
            )
            .map_err(|error| format!("Google read failed: {error}"))?;
            if read == 0 {
                break;
            }
            body_bytes.extend_from_slice(&buffer[..read as usize]);
            if body_bytes.len() > 4 * 1024 * 1024 {
                return Err("Google response is too large".into());
            }
        }

        Ok((status, String::from_utf8_lossy(&body_bytes).to_string()))
    }
}

#[cfg(not(windows))]
fn http_request(
    _method: &str,
    _url: &str,
    _headers: &[(String, String)],
    _body: Option<&[u8]>,
) -> Result<(u32, String), String> {
    Err("Google Calendar is only available on Windows".into())
}
