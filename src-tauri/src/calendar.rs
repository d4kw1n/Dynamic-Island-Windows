//! Read-only calendar support.
//!
//! The island subscribes to an iCalendar feed: a Google Calendar "secret
//! address in iCal format", a published Outlook/iCloud calendar, or a local
//! `.ics` file. Feeds are fetched over WinHTTP with the system proxy, parsed
//! into a small window of upcoming events, and emitted to the UI as
//! `calendar://update`. Reminder scheduling itself lives in the frontend so it
//! can reuse the existing peek + notification path.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use chrono::{
    Datelike, Duration as ChronoDuration, Local, LocalResult, NaiveDate, NaiveDateTime, NaiveTime,
    TimeZone, Weekday,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

#[cfg(windows)]
use url::Url;
#[cfg(windows)]
use windows::core::{HSTRING, PCWSTR};
#[cfg(windows)]
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable,
    WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetOption,
    WinHttpSetTimeouts, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE,
    WINHTTP_OPEN_REQUEST_FLAGS, WINHTTP_OPTION_REDIRECT_POLICY,
    WINHTTP_OPTION_REDIRECT_POLICY_ALWAYS, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
};

const WINDOW_DAYS: i64 = 14;
const MAX_EVENTS: usize = 80;
const MAX_FEED_BYTES: usize = 4 * 1024 * 1024;

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEvent {
    pub id: String,
    pub title: String,
    pub start: i64,
    pub end: i64,
    pub all_day: bool,
    pub location: String,
    pub calendar: String,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct CalendarSnapshot {
    pub source: String,
    pub events: Vec<CalendarEvent>,
    pub updated_at: Option<i64>,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct CalendarState {
    source: Mutex<String>,
    previous: Mutex<Option<CalendarSnapshot>>,
    emitted: Mutex<String>,
}

pub fn start(app: AppHandle) {
    let _ = std::thread::Builder::new()
        .name("island-calendar".into())
        .spawn(move || loop {
            let source = app
                .state::<CalendarState>()
                .source
                .lock()
                .map(|slot| slot.clone())
                .unwrap_or_default();
            if !source.is_empty() {
                refresh(&app, &source);
            } else if crate::google::connected(&app) {
                crate::google::sync(&app);
            }
            std::thread::sleep(Duration::from_secs(45));
        });
}

/// The configured iCal feed (empty when Google Calendar is used instead).
pub(crate) fn current_source(app: &AppHandle) -> String {
    app.state::<CalendarState>()
        .source
        .lock()
        .map(|slot| slot.clone())
        .unwrap_or_default()
}

/// Publishes events fetched through the Google Calendar API. iCal feeds take
/// precedence when both are configured.
pub(crate) fn publish_google(app: &AppHandle, result: Result<Vec<CalendarEvent>, String>) {
    if !current_source(app).is_empty() {
        return;
    }
    publish(app, "Google Calendar", result);
}

pub(crate) fn sync_google(app: &AppHandle) {
    crate::google::sync(app);
}

/// Clears the snapshot after the Google account has been disconnected.
pub(crate) fn clear_google(app: &AppHandle) {
    if !current_source(app).is_empty() {
        return;
    }
    let state = app.state::<CalendarState>();
    let snapshot = CalendarSnapshot::default();
    if let Ok(mut previous) = state.previous.lock() {
        *previous = Some(snapshot.clone());
    }
    if let Ok(mut emitted) = state.emitted.lock() {
        emitted.clear();
    }
    let _ = app.emit("calendar://update", &snapshot);
}

#[tauri::command]
pub fn calendar_set_source(app: AppHandle, source: String) {
    let source = source.trim().to_string();
    {
        let state = app.state::<CalendarState>();
        let Ok(mut slot) = state.source.lock() else {
            return;
        };
        if *slot == source {
            return;
        }
        *slot = source.clone();
    }

    if source.is_empty() {
        // Falling back to Google Calendar when it is connected.
        if crate::google::connected(&app) {
            spawn_refresh(app);
            return;
        }
        let state = app.state::<CalendarState>();
        let snapshot = CalendarSnapshot::default();
        if let Ok(mut previous) = state.previous.lock() {
            *previous = Some(snapshot.clone());
        }
        if let Ok(mut emitted) = state.emitted.lock() {
            emitted.clear();
        }
        let _ = app.emit("calendar://update", &snapshot);
        return;
    }

    spawn_refresh(app);
}

#[tauri::command]
pub fn calendar_refresh(app: AppHandle) {
    spawn_refresh(app);
}

/// Debug helper: parse an .ics file and write the result to
/// `%TEMP%\island-ics-check.json`. Used to validate recurrence handling
/// without a live calendar feed.
#[cfg(debug_assertions)]
pub fn debug_dump(path: &str) {
    let output = match fs::read_to_string(path) {
        Ok(text) => {
            let events = parse_calendar(&text, Local::now().timestamp_millis());
            serde_json::to_string_pretty(&events).unwrap_or_else(|error| error.to_string())
        }
        Err(error) => format!("{{\"error\":\"{error}\"}}"),
    };
    let target = std::env::temp_dir().join("island-ics-check.json");
    let _ = fs::write(target, output);
}

fn spawn_refresh(app: AppHandle) {
    let _ = std::thread::Builder::new()
        .name("island-calendar-sync".into())
        .spawn(move || {
            let source = app
                .state::<CalendarState>()
                .source
                .lock()
                .map(|slot| slot.clone())
                .unwrap_or_default();
            if !source.is_empty() {
                refresh(&app, &source);
            } else if crate::google::connected(&app) {
                crate::google::sync(&app);
            }
        });
}

fn refresh(app: &AppHandle, source: &str) {
    let result = load_source(source).map(|text| parse_calendar(&text, Local::now().timestamp_millis()));
    publish(app, source, result);
}

fn publish(app: &AppHandle, source: &str, result: Result<Vec<CalendarEvent>, String>) {
    let state = app.state::<CalendarState>();
    let mut snapshot = state
        .previous
        .lock()
        .ok()
        .and_then(|slot| slot.clone())
        .unwrap_or_default();
    snapshot.source = source.to_string();

    match result {
        Ok(events) => {
            snapshot.events = events;
            snapshot.updated_at = Some(Local::now().timestamp_millis());
            snapshot.error = None;
        }
        Err(error) => {
            // Keep the last good schedule visible while the network is down.
            snapshot.error = Some(error);
        }
    }

    let json = serde_json::to_string(&snapshot).unwrap_or_default();
    {
        let Ok(mut emitted) = state.emitted.lock() else {
            return;
        };
        if *emitted == json {
            return;
        }
        *emitted = json;
    }
    if let Ok(mut previous) = state.previous.lock() {
        *previous = Some(snapshot.clone());
    }
    let _ = app.emit("calendar://update", &snapshot);
}

// ---------------------------------------------------------------------------
// Feed loading
// ---------------------------------------------------------------------------

fn load_source(source: &str) -> Result<String, String> {
    let source = source.trim();
    if source.is_empty() {
        return Err("No calendar connected".into());
    }
    if source.starts_with("http://")
        || source.starts_with("https://")
        || source.starts_with("webcal://")
    {
        return http_get(source);
    }

    let mut path = source.to_string();
    if let Some(rest) = path.strip_prefix("file://").map(str::to_string) {
        path = rest;
    }
    // file:///D:/calendar.ics -> /D:/calendar.ics
    if path.starts_with('/') && path.len() > 3 && path.as_bytes()[2] == b':' {
        path.remove(0);
    }
    if path.is_empty() {
        return Err("No calendar connected".into());
    }
    fs::read_to_string(Path::new(&path)).map_err(|error| format!("Cannot read {path}: {error}"))
}

#[cfg(windows)]
struct InternetHandle(*mut core::ffi::c_void);

#[cfg(windows)]
impl Drop for InternetHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

#[cfg(windows)]
fn http_get(source: &str) -> Result<String, String> {
    let target = match source.strip_prefix("webcal://") {
        Some(rest) => format!("https://{rest}"),
        None => source.to_string(),
    };
    let url = Url::parse(&target).map_err(|error| format!("Invalid calendar URL: {error}"))?;
    let secure = match url.scheme() {
        "https" => true,
        "http" => false,
        other => return Err(format!("Unsupported calendar scheme: {other}")),
    };
    let host = url
        .host_str()
        .ok_or("Calendar URL has no host")?
        .to_string();
    let port = url
        .port_or_known_default()
        .unwrap_or(if secure { 443 } else { 80 });

    let mut object = url.path().to_string();
    if let Some(query) = url.query() {
        object.push('?');
        object.push_str(query);
    }
    if object.is_empty() {
        object.push('/');
    }

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
        let _ = WinHttpSetTimeouts(session.0, 6000, 6000, 8000, 12000);
        let redirect: u32 = WINHTTP_OPTION_REDIRECT_POLICY_ALWAYS;
        let _ = WinHttpSetOption(
            Some(session.0 as *const _),
            WINHTTP_OPTION_REDIRECT_POLICY,
            Some(&redirect.to_le_bytes()),
        );

        let host_w = HSTRING::from(host);
        let object_w = HSTRING::from(object);
        let verb = HSTRING::from("GET");

        let connect = WinHttpConnect(session.0, &host_w, port as u16, 0);
        if connect.is_null() {
            return Err("Cannot connect to the calendar host".into());
        }
        let connect = InternetHandle(connect);

        let request = WinHttpOpenRequest(
            connect.0,
            &verb,
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
            return Err("Cannot open the calendar request".into());
        }
        let request = InternetHandle(request);

        WinHttpSendRequest(request.0, None, None, 0, 0, 0)
            .map_err(|error| format!("Calendar request failed: {error}"))?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut())
            .map_err(|error| format!("Calendar host did not answer: {error}"))?;

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
        if status >= 400 {
            return Err(format!("Calendar feed returned HTTP {status}"));
        }

        let mut body: Vec<u8> = Vec::new();
        loop {
            let mut available: u32 = 0;
            WinHttpQueryDataAvailable(request.0, &mut available)
                .map_err(|error| format!("Calendar read failed: {error}"))?;
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
            .map_err(|error| format!("Calendar read failed: {error}"))?;
            if read == 0 {
                break;
            }
            body.extend_from_slice(&buffer[..read as usize]);
            if body.len() > MAX_FEED_BYTES {
                return Err("Calendar feed is too large".into());
            }
        }

        Ok(String::from_utf8_lossy(&body).to_string())
    }
}

#[cfg(not(windows))]
fn http_get(_source: &str) -> Result<String, String> {
    Err("Calendar downloads are only supported on Windows".into())
}

// ---------------------------------------------------------------------------
// iCalendar parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
struct Prop {
    name: String,
    params: Vec<(String, String)>,
    value: String,
}

impl Prop {
    fn param(&self, key: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }
}

#[derive(Default)]
struct RawEvent {
    props: Vec<Prop>,
}

impl RawEvent {
    fn get(&self, name: &str) -> Option<&Prop> {
        self.props.iter().find(|prop| prop.name == name)
    }

    fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Prop> + 'a {
        self.props.iter().filter(move |prop| prop.name == name)
    }
}

fn unfold(text: &str) -> Vec<String> {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let text = text.replace('\r', "\n");
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        if raw.starts_with(' ') || raw.starts_with('\t') {
            if let Some(last) = lines.last_mut() {
                last.push_str(&raw[1..]);
            }
        } else {
            lines.push(raw.to_string());
        }
    }
    lines
}

fn parse_prop(line: &str) -> Option<Prop> {
    let split = line.find(':')?;
    let (left, right) = line.split_at(split);
    let value = right[1..].to_string();

    let mut pieces = left.split(';');
    let name = pieces.next()?.trim().to_ascii_uppercase();
    if name.is_empty() {
        return None;
    }

    let mut params = Vec::new();
    for piece in pieces {
        if let Some((key, raw)) = piece.split_once('=') {
            params.push((
                key.trim().to_ascii_uppercase(),
                raw.trim().trim_matches('"').to_string(),
            ));
        }
    }

    Some(Prop {
        name,
        params,
        value,
    })
}

fn parse_calendar(text: &str, now: i64) -> Vec<CalendarEvent> {
    let window_start = now - 30 * 60_000;
    let window_end = now + WINDOW_DAYS * 86_400_000;

    let mut events: Vec<CalendarEvent> = Vec::new();
    let mut current: Option<RawEvent> = None;
    let mut calendar_name = String::new();

    for line in unfold(text) {
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case("BEGIN:VEVENT") {
            current = Some(RawEvent::default());
            continue;
        }
        if trimmed.eq_ignore_ascii_case("END:VEVENT") {
            if let Some(raw) = current.take() {
                events.extend(build_events(
                    &raw,
                    &calendar_name,
                    window_start,
                    window_end,
                ));
            }
            continue;
        }
        match current.as_mut() {
            Some(raw) => {
                if let Some(prop) = parse_prop(trimmed) {
                    raw.props.push(prop);
                }
            }
            None => {
                if let Some(prop) = parse_prop(trimmed) {
                    if prop.name == "X-WR-CALNAME" {
                        calendar_name = unescape(&prop.value);
                    }
                }
            }
        }
    }

    let mut seen: HashSet<(String, i64)> = HashSet::new();
    events.retain(|event| seen.insert((event.title.to_lowercase(), event.start)));
    events.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| a.title.cmp(&b.title)));
    events.truncate(MAX_EVENTS);
    events
}

fn build_events(
    raw: &RawEvent,
    calendar_name: &str,
    window_start: i64,
    window_end: i64,
) -> Vec<CalendarEvent> {
    let Some(start_prop) = raw.get("DTSTART") else {
        return Vec::new();
    };
    let tzid = start_prop.param("TZID");
    let Some((start, all_day)) = parse_date_with_tz(&start_prop.value, tzid) else {
        return Vec::new();
    };
    if raw
        .get("STATUS")
        .map(|prop| prop.value.eq_ignore_ascii_case("CANCELLED"))
        .unwrap_or(false)
    {
        return Vec::new();
    }

    let end = raw
        .get("DTEND")
        .and_then(|prop| parse_date_with_tz(&prop.value, prop.param("TZID")).map(|(ms, _)| ms))
        .or_else(|| {
            raw.get("DURATION")
                .and_then(|prop| parse_duration(&prop.value))
                .map(|duration| start + duration)
        })
        .unwrap_or(if all_day {
            start + 86_400_000
        } else {
            start + 3_600_000
        });
    let duration = (end - start).max(if all_day { 86_400_000 } else { 60_000 });

    let title = raw
        .get("SUMMARY")
        .map(|prop| unescape(&prop.value))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Untitled event".into());
    let location = raw
        .get("LOCATION")
        .map(|prop| unescape(&prop.value))
        .unwrap_or_default();
    let uid = raw
        .get("UID")
        .map(|prop| prop.value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("{title}:{start}"));

    let base = CalendarEvent {
        id: uid.clone(),
        title,
        start,
        end,
        all_day,
        location,
        calendar: calendar_name.to_string(),
    };

    if let Some(rule) = raw.get("RRULE") {
        let exdates: HashSet<i64> = raw
            .all("EXDATE")
            .flat_map(|prop| prop.value.split(','))
            .filter_map(|value| parse_date_value(value).map(|(ms, _)| ms))
            .collect();
        return expand_rrule(start, duration, &rule.value, &exdates, window_start, window_end)
            .into_iter()
            .map(|(slot_start, slot_end)| CalendarEvent {
                id: format!("{uid}:{slot_start}"),
                start: slot_start,
                end: slot_end,
                ..base.clone()
            })
            .collect();
    }

    if end >= window_start && start <= window_end {
        vec![CalendarEvent {
            id: format!("{uid}:{start}"),
            ..base
        }]
    } else {
        Vec::new()
    }
}

fn parse_date_value(value: &str) -> Option<(i64, bool)> {
    parse_date_with_tz(value, None)
}

fn parse_date_with_tz(value: &str, tzid: Option<&str>) -> Option<(i64, bool)> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if value.len() == 8 && !value.contains('T') {
        let date = NaiveDate::parse_from_str(value, "%Y%m%d").ok()?;
        return Some((local_ms(date.and_hms_opt(0, 0, 0)?), true));
    }

    let utc = value.ends_with('Z')
        || tzid
            .map(|zone| zone.eq_ignore_ascii_case("UTC") || zone.eq_ignore_ascii_case("Etc/UTC"))
            .unwrap_or(false);
    let core = value.trim_end_matches('Z');
    let format = if core.len() == 13 {
        "%Y%m%dT%H%M"
    } else {
        "%Y%m%dT%H%M%S"
    };
    let naive = NaiveDateTime::parse_from_str(core, format).ok()?;
    let ms = if utc {
        naive.and_utc().timestamp_millis()
    } else {
        // Named TZIDs are interpreted as local wall time. Reminders care about
        // the user's own clock, and mainstream feeds export their events in
        // the account's timezone, which is the Windows timezone here.
        local_ms(naive)
    };
    Some((ms, false))
}

fn local_ms(naive: NaiveDateTime) -> i64 {
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(value) => value.timestamp_millis(),
        LocalResult::Ambiguous(first, _) => first.timestamp_millis(),
        LocalResult::None => naive.and_utc().timestamp_millis(),
    }
}

fn parse_duration(value: &str) -> Option<i64> {
    let value = value.trim().to_ascii_uppercase();
    let negative = value.starts_with('-');
    let value = value.trim_start_matches(['+', '-']);
    let mut rest = value.strip_prefix('P')?.chars().peekable();

    let mut total: i64 = 0;
    let mut number = String::new();
    let mut in_time = false;

    while let Some(ch) = rest.next() {
        if ch.is_ascii_digit() {
            number.push(ch);
            continue;
        }
        if ch == 'T' {
            in_time = true;
            number.clear();
            continue;
        }
        let amount: i64 = number.parse().ok()?;
        number.clear();
        total += match ch {
            'W' => amount * 7 * 86_400_000,
            'D' => amount * 86_400_000,
            'H' => amount * 3_600_000,
            'M' if in_time => amount * 60_000,
            'S' => amount * 1_000,
            _ => return None,
        };
    }

    Some(if negative { -total } else { total })
}

fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') | Some('N') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => break,
        }
    }
    out.trim().to_string()
}

// ---------------------------------------------------------------------------
// Recurrence expansion
// ---------------------------------------------------------------------------

struct Expansion {
    duration: i64,
    window_start: i64,
    window_end: i64,
    exdates: HashSet<i64>,
    count: Option<i64>,
    until: Option<i64>,
    generated: i64,
    out: Vec<(i64, i64)>,
}

impl Expansion {
    /// Adds one occurrence. Returns false when the series has ended.
    fn emit(&mut self, candidate: NaiveDateTime) -> bool {
        let ms = local_ms(candidate);
        if ms > self.window_end {
            return false;
        }
        if let Some(until) = self.until {
            if ms > until {
                return false;
            }
        }
        self.generated += 1;
        if let Some(count) = self.count {
            if self.generated > count {
                return false;
            }
        }
        if ms >= self.window_start && !self.exdates.contains(&ms) && self.out.len() < 400 {
            self.out.push((ms, ms + self.duration));
        }
        true
    }
}

fn expand_rrule(
    start_ms: i64,
    duration: i64,
    rule: &str,
    exdates: &HashSet<i64>,
    window_start: i64,
    window_end: i64,
) -> Vec<(i64, i64)> {
    let start_local = match Local.timestamp_millis_opt(start_ms) {
        LocalResult::Single(value) => value.naive_local(),
        LocalResult::Ambiguous(value, _) => value.naive_local(),
        LocalResult::None => return Vec::new(),
    };

    let parts: HashMap<String, String> = rule
        .split(';')
        .filter_map(|part| part.split_once('='))
        .map(|(key, value)| (key.trim().to_ascii_uppercase(), value.trim().to_ascii_uppercase()))
        .collect();

    let frequency = parts.get("FREQ").map(String::as_str).unwrap_or("DAILY");
    let interval = parts
        .get("INTERVAL")
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(1)
        .max(1);
    let count = parts.get("COUNT").and_then(|value| value.parse::<i64>().ok());
    let until = parts.get("UNTIL").and_then(|value| parse_until(value));
    let byday: Vec<Weekday> = parts
        .get("BYDAY")
        .map(|value| {
            value
                .split(',')
                .filter_map(weekday_of)
                .collect::<Vec<Weekday>>()
        })
        .unwrap_or_default();
    let bymonthday: Vec<i32> = parts
        .get("BYMONTHDAY")
        .map(|value| {
            value
                .split(',')
                .filter_map(|part| part.parse::<i32>().ok())
                .filter(|day| *day != 0 && day.abs() <= 31)
                .collect::<Vec<i32>>()
        })
        .unwrap_or_default();

    let mut expansion = Expansion {
        duration,
        window_start,
        window_end,
        exdates: exdates.clone(),
        count,
        until,
        generated: 0,
        out: Vec::new(),
    };
    let start_date = start_local.date();
    let start_time = start_local.time();

    match frequency {
        "WEEKLY" if !byday.is_empty() => {
            let mut days: Vec<u32> = byday
                .iter()
                .map(|day| day.num_days_from_monday())
                .collect();
            days.sort_unstable();
            days.dedup();
            let mut week_start = start_date
                - ChronoDuration::days(start_date.weekday().num_days_from_monday() as i64);
            let mut guard = 0;
            'outer: loop {
                guard += 1;
                if guard > 4000 {
                    break;
                }
                for offset in &days {
                    let date = week_start + ChronoDuration::days(*offset as i64);
                    let candidate = NaiveDateTime::new(date, start_time);
                    if candidate < start_local {
                        continue;
                    }
                    if !expansion.emit(candidate) {
                        break 'outer;
                    }
                }
                week_start += ChronoDuration::weeks(interval);
                let probe = NaiveDateTime::new(week_start, start_time);
                if local_ms(probe) > window_end {
                    break;
                }
            }
        }
        "WEEKLY" => {
            let mut cursor = start_local;
            let mut guard = 0;
            loop {
                guard += 1;
                if guard > 4000 || !expansion.emit(cursor) {
                    break;
                }
                cursor += ChronoDuration::weeks(interval);
            }
        }
        "MONTHLY" => {
            let (mut year, mut month) = (start_date.year(), start_date.month());
            let mut guard = 0;
            'outer: loop {
                guard += 1;
                if guard > 4000 {
                    break;
                }
                let mut dates: Vec<NaiveDate> = Vec::new();
                if bymonthday.is_empty() {
                    if let Some(date) = clamp_date(year, month, start_date.day()) {
                        dates.push(date);
                    }
                } else {
                    let last = days_in_month(year, month);
                    for day in &bymonthday {
                        let resolved = if *day > 0 {
                            *day as u32
                        } else {
                            last.saturating_sub(((-*day) as u32).saturating_sub(1))
                        };
                        if resolved >= 1 && resolved <= last {
                            if let Some(date) = NaiveDate::from_ymd_opt(year, month, resolved) {
                                dates.push(date);
                            }
                        }
                    }
                }
                dates.sort_unstable();
                dates.dedup();

                for date in dates {
                    let candidate = NaiveDateTime::new(date, start_time);
                    if candidate < start_local {
                        continue;
                    }
                    if !expansion.emit(candidate) {
                        break 'outer;
                    }
                }

                let (next_year, next_month) = add_months(year, month, interval);
                year = next_year;
                month = next_month;
                if let Some(first) = NaiveDate::from_ymd_opt(year, month, 1) {
                    if local_ms(NaiveDateTime::new(first, NaiveTime::MIN)) > window_end {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
        "YEARLY" => {
            let mut year = start_date.year();
            let mut guard = 0;
            loop {
                guard += 1;
                if guard > 400 {
                    break;
                }
                if let Some(date) = clamp_date(year, start_date.month(), start_date.day()) {
                    let candidate = NaiveDateTime::new(date, start_time);
                    if candidate >= start_local && !expansion.emit(candidate) {
                        break;
                    }
                }
                year += interval as i32;
                let Some(first) = NaiveDate::from_ymd_opt(year, 1, 1) else {
                    break;
                };
                if local_ms(NaiveDateTime::new(first, NaiveTime::MIN)) > window_end {
                    break;
                }
            }
        }
        // Daily is also the fallback for unknown FREQ values: showing a
        // reminder is better than silently dropping the series.
        _ => {
            let mut cursor = start_local;
            let mut guard = 0;
            loop {
                guard += 1;
                if guard > 20_000 || !expansion.emit(cursor) {
                    break;
                }
                cursor += ChronoDuration::days(interval);
            }
        }
    }

    expansion.out
}

fn parse_until(value: &str) -> Option<i64> {
    let (ms, all_day) = parse_date_value(value)?;
    Some(if all_day { ms + 86_400_000 - 1 } else { ms })
}

fn weekday_of(code: &str) -> Option<Weekday> {
    let code = code.trim();
    let tail = code.get(code.len().saturating_sub(2)..)?.to_ascii_uppercase();
    match tail.as_str() {
        "MO" => Some(Weekday::Mon),
        "TU" => Some(Weekday::Tue),
        "WE" => Some(Weekday::Wed),
        "TH" => Some(Weekday::Thu),
        "FR" => Some(Weekday::Fri),
        "SA" => Some(Weekday::Sat),
        "SU" => Some(Weekday::Sun),
        _ => None,
    }
}

fn add_months(year: i32, month: u32, add: i64) -> (i32, u32) {
    let total = year as i64 * 12 + (month as i64 - 1) + add;
    (
        total.div_euclid(12) as i32,
        (total.rem_euclid(12) + 1) as u32,
    )
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = add_months(year, month, 1);
    NaiveDate::from_ymd_opt(next_year, next_month, 1)
        .and_then(|date| date.pred_opt())
        .map(|date| date.day())
        .unwrap_or(28)
}

fn clamp_date(year: i32, month: u32, day: u32) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(year, month, day.min(days_in_month(year, month)))
}
