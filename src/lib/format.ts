export function fmtDuration(ms: number): string {
  const total = Math.max(0, Math.round(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  if (h > 0) {
    return `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
  }
  return `${m}:${String(s).padStart(2, "0")}`;
}

export function fmtClock(ts: number): string {
  const d = new Date(ts);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

export function fmtElapsed(ms: number): string {
  const total = Math.max(0, Math.round(ms / 1000));
  if (total < 60) return `${total}s`;
  const m = Math.floor(total / 60);
  const s = total % 60;
  if (m < 60) return s === 0 ? `${m}m` : `${m}m ${String(s).padStart(2, "0")}s`;
  const h = Math.floor(m / 60);
  const rm = m % 60;
  return rm === 0 ? `${h}h` : `${h}h ${String(rm).padStart(2, "0")}m`;
}

export function fmtAgo(ts: number, now: number): string {
  const diff = Math.max(0, now - ts);
  if (diff < 60_000) return "just now";
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h ago`;
  return `${Math.floor(diff / 86_400_000)}d ago`;
}

export function sourceLabel(raw: string): string {
  const value = (raw || "").toLowerCase();
  if (!value) return "Media";
  if (value.includes("spotify")) return "Spotify";
  if (value.includes("msedge")) return "Edge";
  if (value.includes("chrome")) return "Chrome";
  if (value.includes("brave")) return "Brave";
  if (value.includes("firefox")) return "Firefox";
  if (value.includes("applemusic") || value.includes("music.apple")) return "Apple Music";
  if (value.includes("vlc")) return "VLC";
  if (value.includes("foobar")) return "foobar2000";
  if (value.includes("tidal")) return "TIDAL";
  if (value.includes("zune")) return "Groove";
  const file = raw.split(/[\\/]/).pop() || raw;
  return file.replace(/\.exe$/i, "");
}

export function statusLabel(status: string): string {
  switch (status) {
    case "playing":
      return "Playing";
    case "paused":
      return "Paused";
    case "stopped":
      return "Stopped";
    case "changing":
      return "Buffering";
    default:
      return "";
  }
}
