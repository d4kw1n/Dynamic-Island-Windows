//! Windows System Media Transport Controls (SMTC) integration.
//!
//! Works with Spotify, browsers (YouTube, SoundCloud), Apple Music and any
//! other app that publishes a media session to Windows — no OAuth or API keys.

use std::sync::{
    mpsc::{channel, Receiver, RecvTimeoutError, Sender},
    Mutex,
};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use windows::core::Interface;
use windows::Foundation::TimeSpan;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession,
    GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionMediaProperties,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus,
};
use windows::Storage::Streams::{DataReader, IRandomAccessStream};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

use base64::Engine;
use futures::executor::block_on;
use std::future::IntoFuture;

pub enum MediaCmd {
    PlayPause,
    Next,
    Previous,
    Seek(i64),
}

#[derive(Default)]
pub struct MediaState {
    tx: Mutex<Option<Sender<MediaCmd>>>,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct MediaSnapshot {
    active: bool,
    title: String,
    artist: String,
    album: String,
    source: String,
    playing: bool,
    status: String,
    position_ms: i64,
    duration_ms: i64,
    track_key: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct MediaArtwork {
    key: String,
    data: String,
}

pub fn start(app: AppHandle) {
    let state = app.state::<MediaState>();
    let (tx, rx) = channel::<MediaCmd>();
    if let Ok(mut guard) = state.tx.lock() {
        *guard = Some(tx);
    }
    std::thread::Builder::new()
        .name("island-media".into())
        .spawn(move || media_loop(app, rx))
        .ok();
}

#[tauri::command]
pub fn media_control(
    action: String,
    position_ms: Option<i64>,
    state: tauri::State<'_, MediaState>,
) -> Result<(), String> {
    let guard = state.tx.lock().map_err(|e| e.to_string())?;
    let tx = guard.as_ref().ok_or("media engine is not ready")?;
    let cmd = match action.as_str() {
        "play_pause" => MediaCmd::PlayPause,
        "next" => MediaCmd::Next,
        "previous" => MediaCmd::Previous,
        "seek" => MediaCmd::Seek(position_ms.unwrap_or(0).max(0)),
        other => return Err(format!("unknown media action: {other}")),
    };
    tx.send(cmd).map_err(|e| e.to_string())
}

fn media_loop(app: AppHandle, rx: Receiver<MediaCmd>) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }

    let mut manager: Option<GlobalSystemMediaTransportControlsSessionManager> = None;
    let mut last_signature = String::from("__init__");
    let mut last_art_key = String::new();
    let mut art_attempts = 0u8;
    let mut last_emit = Instant::now() - Duration::from_secs(5);

    loop {
        if manager.is_none() {
            match GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
                .and_then(|op| block_on(op.into_future()))
            {
                Ok(m) => manager = Some(m),
                Err(_) => {
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                }
            }
        }

        let m = manager.as_ref().expect("manager is initialized");
        let (snapshot, artwork) = poll(m, &last_art_key);

        let signature = format!(
            "{}|{}|{}|{}|{}|{}|{}|{}",
            snapshot.active,
            snapshot.title,
            snapshot.artist,
            snapshot.album,
            snapshot.source,
            snapshot.playing,
            snapshot.status,
            snapshot.duration_ms
        );

        let due = last_emit.elapsed() >= Duration::from_millis(900);
        let active_tick = snapshot.active && (snapshot.playing || due);
        if signature != last_signature || active_tick {
            let _ = app.emit("media://update", &snapshot);
            last_signature = signature;
            last_emit = Instant::now();
        }

        if let Some(art) = artwork {
            let _ = app.emit("media://artwork", &art);
            last_art_key = art.key;
            art_attempts = 0;
        } else if snapshot.active && snapshot.track_key != last_art_key {
            art_attempts += 1;
            if art_attempts >= 4 {
                // Give up for this track (some apps never publish artwork).
                last_art_key = snapshot.track_key.clone();
                art_attempts = 0;
            }
        }

        match rx.recv_timeout(Duration::from_millis(650)) {
            Ok(cmd) => handle_cmd(m, cmd),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn poll(
    m: &GlobalSystemMediaTransportControlsSessionManager,
    last_art_key: &str,
) -> (MediaSnapshot, Option<MediaArtwork>) {
    let session = match m.GetCurrentSession() {
        Ok(s) => s,
        Err(_) => return (MediaSnapshot::default(), None),
    };

    let props = match session
        .TryGetMediaPropertiesAsync()
        .and_then(|op| block_on(op.into_future()))
    {
        Ok(p) => p,
        Err(_) => return (MediaSnapshot::default(), None),
    };

    let title = hstring(props.Title());
    let artist = hstring(props.Artist());
    let album = hstring(props.AlbumTitle());
    let source = hstring(session.SourceAppUserModelId());
    if title.is_empty() && artist.is_empty() {
        return (MediaSnapshot::default(), None);
    }

    let track_key = format!("{source}|{title}|{artist}|{album}");

    let (playing, status) = match session
        .GetPlaybackInfo()
        .and_then(|info| info.PlaybackStatus())
    {
        Ok(GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing) => (true, "playing"),
        Ok(GlobalSystemMediaTransportControlsSessionPlaybackStatus::Changing) => {
            (true, "changing")
        }
        Ok(GlobalSystemMediaTransportControlsSessionPlaybackStatus::Paused) => (false, "paused"),
        Ok(GlobalSystemMediaTransportControlsSessionPlaybackStatus::Stopped) => (false, "stopped"),
        _ => (false, "unknown"),
    };

    let (position_ms, duration_ms) = timeline(&session);

    let artwork = if track_key != last_art_key {
        read_artwork(&props).map(|data| MediaArtwork {
            key: track_key.clone(),
            data,
        })
    } else {
        None
    };

    (
        MediaSnapshot {
            active: true,
            title,
            artist,
            album,
            source,
            playing,
            status: status.to_string(),
            position_ms,
            duration_ms,
            track_key,
        },
        artwork,
    )
}

fn timeline(session: &GlobalSystemMediaTransportControlsSession) -> (i64, i64) {
    let props = match session.GetTimelineProperties() {
        Ok(p) => p,
        Err(_) => return (0, 0),
    };
    let start = props.StartTime().map(|t| t.duration()).unwrap_or(0);
    let end = props.EndTime().map(|t| t.duration()).unwrap_or(0);
    let pos = props.Position().map(|t| t.duration()).unwrap_or(0);

    let duration = ((end - start) / 10_000).max(0);
    let position = ((pos - start) / 10_000).clamp(0, duration.max(0));
    (position, duration)
}

trait TimeSpanExt {
    fn duration(self) -> i64;
}

impl TimeSpanExt for TimeSpan {
    fn duration(self) -> i64 {
        self.Duration
    }
}

fn read_artwork(
    props: &GlobalSystemMediaTransportControlsSessionMediaProperties,
) -> Option<String> {
    let thumb = props.Thumbnail().ok()?;
    let stream = block_on(thumb.OpenReadAsync().ok()?.into_future()).ok()?;
    let size = stream.Size().ok()?;
    if size == 0 || size > 8 * 1024 * 1024 {
        return None;
    }
    let base: IRandomAccessStream = stream.cast().ok()?;
    let reader = DataReader::CreateDataReader(&base).ok()?;
    let _ = block_on(reader.LoadAsync(size as u32).ok()?.into_future()).ok()?;

    let mut buf = vec![0u8; size as usize];
    let _ = reader.ReadBytes(&mut buf).ok()?;

    let mime = if buf.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if buf.starts_with(&[0xFF, 0xD8]) {
        "image/jpeg"
    } else if buf.starts_with(b"GIF8") {
        "image/gif"
    } else if buf.starts_with(&[0x52, 0x49, 0x46, 0x46]) {
        "image/webp"
    } else {
        "image/jpeg"
    };

    let encoded = base64::engine::general_purpose::STANDARD.encode(&buf);
    Some(format!("data:{mime};base64,{encoded}"))
}

fn hstring(value: windows::core::Result<windows::core::HSTRING>) -> String {
    value.map(|s| s.to_string()).unwrap_or_default()
}

fn handle_cmd(m: &GlobalSystemMediaTransportControlsSessionManager, cmd: MediaCmd) {
    let session = match m.GetCurrentSession() {
        Ok(s) => s,
        Err(_) => return,
    };
    match cmd {
        MediaCmd::PlayPause => {
            let _ = session.TryTogglePlayPauseAsync().and_then(|op| block_on(op.into_future()));
        }
        MediaCmd::Next => {
            let _ = session.TrySkipNextAsync().and_then(|op| block_on(op.into_future()));
        }
        MediaCmd::Previous => {
            let _ = session.TrySkipPreviousAsync().and_then(|op| block_on(op.into_future()));
        }
        MediaCmd::Seek(ms) => {
            let _ = session.TryChangePlaybackPositionAsync(ms * 10_000).and_then(|op| block_on(op.into_future()));
        }
    }
}
