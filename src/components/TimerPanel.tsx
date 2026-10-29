import { useState, type FormEvent } from "react";
import { Pause, Play, RotateCcw } from "lucide-react";
import { useNow } from "../hooks/useNow";
import { fmtDuration } from "../lib/format";
import {
  timerProgress,
  timerRemaining,
  useStore,
} from "../store/useStore";
import { Ring } from "./Bits";

const PRESETS = [1, 5, 10, 25, 45];

export function TimerPanel() {
  const timer = useStore((s) => s.timer);
  const startTimer = useStore((s) => s.startTimer);
  const pauseTimer = useStore((s) => s.pauseTimer);
  const resumeTimer = useStore((s) => s.resumeTimer);
  const resetTimer = useStore((s) => s.resetTimer);

  const now = useNow(500);
  const [custom, setCustom] = useState("");

  const remaining = timerRemaining(timer, now);
  const progress = timerProgress(timer, now);
  const finished = remaining <= 0;
  const paused = !timer.running && timer.remainingMs > 0 && timer.remainingMs < timer.durationMs;

  const status = timer.running
    ? "Focusing"
    : finished
      ? "Complete"
      : paused
        ? "Paused"
        : "Ready";
  const color = finished ? "#30d158" : "#ff9f0a";

  const mainLabel = timer.running
    ? "Pause"
    : finished
      ? "Restart"
      : paused
        ? "Resume"
        : "Start";

  const onMain = () => {
    if (timer.running) {
      pauseTimer();
      return;
    }
    if (finished) {
      startTimer(timer.durationMs || 25 * 60_000);
      return;
    }
    resumeTimer();
  };

  const onCustom = (event: FormEvent) => {
    event.preventDefault();
    const minutes = Number(custom.replace(",", "."));
    if (!Number.isFinite(minutes) || minutes <= 0) return;
    startTimer(Math.min(240, minutes) * 60_000);
    setCustom("");
  };

  return (
    <div className="timer-panel">
      <div className="timer-left">
        <Ring progress={progress} color={color} size={108} stroke={5}>
          <div className="timer-time">{fmtDuration(remaining)}</div>
          <div className="timer-state" style={{ color }}>
            {status}
          </div>
        </Ring>
        <div className="timer-caption">
          {Math.max(1, Math.round(timer.durationMs / 60_000))} min session
        </div>
      </div>

      <div className="timer-right">
        <div className="presets">
          {PRESETS.map((minutes) => (
            <button
              key={minutes}
              className="chip"
              onClick={() => startTimer(minutes * 60_000)}
            >
              {minutes}m
            </button>
          ))}
        </div>

        <div className="timer-actions">
          <button className="btn-primary" onClick={onMain}>
            {timer.running ? (
              <Pause size={12} fill="currentColor" strokeWidth={1} />
            ) : (
              <Play size={12} fill="currentColor" strokeWidth={1} />
            )}
            <span>{mainLabel}</span>
          </button>
          <button className="btn-ghost" title="Reset timer" onClick={resetTimer}>
            <RotateCcw size={14} />
          </button>
        </div>

        <form className="custom-row" onSubmit={onCustom}>
          <span>Custom</span>
          <input
            value={custom}
            onChange={(event) => setCustom(event.target.value)}
            placeholder="15"
            inputMode="decimal"
            aria-label="Custom minutes"
          />
          <span>min</span>
          <button type="submit" className="link-btn">
            Set
          </button>
        </form>
      </div>
    </div>
  );
}
