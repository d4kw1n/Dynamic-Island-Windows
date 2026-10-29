import { useRef, useState, type CSSProperties } from "react";
import {
  Music2,
  Pause,
  Play,
  Radio,
  SkipBack,
  SkipForward,
} from "lucide-react";
import { useNow } from "../hooks/useNow";
import { useArtGlow } from "../hooks/useArtGlow";
import { invoke } from "../lib/bridge";
import { fmtDuration, sourceLabel, statusLabel } from "../lib/format";
import { useStore } from "../store/useStore";

export function MediaPanel() {
  const media = useStore((s) => s.media);
  const artworkMap = useStore((s) => s.artwork);
  const artwork = artworkMap[media.trackKey];
  const glow = useArtGlow(artwork);

  const barRef = useRef<HTMLDivElement>(null);
  const [dragging, setDragging] = useState(false);
  const [dragValue, setDragValue] = useState(0);

  // The Rust poll forwards a fresh position about once a second; ticking here
  // keeps the bar and the clock moving smoothly in between.
  const now = useNow(250);
  const live =
    media.positionMs +
    (media.playing ? now - (media.receivedAt ?? now) : 0);
  const position = Math.min(
    Math.max(0, dragging ? dragValue : live),
    media.durationMs || 0,
  );
  const ratio = media.durationMs > 0 ? position / media.durationMs : 0;

  const control = (action: string) => {
    void invoke("media_control", { action }).catch(() => {});
  };

  const ratioAt = (clientX: number): number => {
    const rect = barRef.current?.getBoundingClientRect();
    if (!rect || rect.width === 0) return 0;
    return Math.min(1, Math.max(0, (clientX - rect.left) / rect.width));
  };

  const seekTo = (clientX: number) => {
    setDragValue(ratioAt(clientX) * media.durationMs);
  };

  if (!media.active) {
    return (
      <div className="media-panel">
        <div className="empty">
          <div className="empty-icon">
            <Music2 size={20} />
          </div>
          <div className="empty-title">Nothing playing</div>
          <div className="empty-sub">
            Play something in Spotify, YouTube or any Windows media app and it
            will show up here.
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="media-panel">
      <div
        className="media-glow"
        style={{ "--glow": glow } as CSSProperties}
      />

      <div className="media-top">
        {artwork ? (
          <img className="media-art" src={artwork} alt="" draggable={false} />
        ) : (
          <div className="media-art media-art-fallback">
            <Music2 size={20} />
          </div>
        )}
        <div className="media-meta">
          <div className="media-title">{media.title || "Unknown track"}</div>
          <div className="media-artist">
            {[media.artist, media.album].filter(Boolean).join(" · ") || "—"}
          </div>
          <div className="media-source">
            <Radio size={9} />
            <span>{sourceLabel(media.source)}</span>
            {statusLabel(media.status) && (
              <>
                <span className="dot-sep">·</span>
                <span>{statusLabel(media.status)}</span>
              </>
            )}
          </div>
        </div>
      </div>

      <div
        className={`seek ${media.durationMs > 0 ? "" : "disabled"}`}
        ref={barRef}
        onPointerDown={(event) => {
          if (media.durationMs <= 0) return;
          event.currentTarget.setPointerCapture(event.pointerId);
          setDragging(true);
          seekTo(event.clientX);
        }}
        onPointerMove={(event) => {
          if (dragging) seekTo(event.clientX);
        }}
        onPointerUp={(event) => {
          if (!dragging) return;
          const next = ratioAt(event.clientX) * media.durationMs;
          setDragging(false);
          void invoke("media_control", {
            action: "seek",
            positionMs: Math.round(next),
          }).catch(() => {});
        }}
        onPointerCancel={() => setDragging(false)}
      >
        <div className="seek-track">
          <div
            className="seek-fill"
            style={{
              width: `${ratio * 100}%`,
              transition:
                media.playing && !dragging ? "width .25s linear" : "none",
            }}
          />
        </div>
      </div>

      <div className="media-times">
        <span>{fmtDuration(position)}</span>
        <span>{fmtDuration(media.durationMs)}</span>
      </div>

      <div className="media-controls">
        <button
          className="icon-btn"
          title="Previous"
          onClick={() => control("previous")}
        >
          <SkipBack size={16} fill="currentColor" strokeWidth={1.2} />
        </button>
        <button
          className="play-btn"
          title={media.playing ? "Pause" : "Play"}
          onClick={() => control("play_pause")}
        >
          {media.playing ? (
            <Pause size={15} fill="currentColor" strokeWidth={1} />
          ) : (
            <Play size={15} fill="currentColor" strokeWidth={1} />
          )}
        </button>
        <button
          className="icon-btn"
          title="Next"
          onClick={() => control("next")}
        >
          <SkipForward size={16} fill="currentColor" strokeWidth={1.2} />
        </button>
      </div>
    </div>
  );
}
