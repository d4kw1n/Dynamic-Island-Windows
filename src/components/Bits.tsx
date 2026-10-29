import { Check } from "lucide-react";
import type { ReactNode } from "react";
import { brandFor } from "../lib/agents";

export function Monogram({ agent, size = 30 }: { agent: string; size?: number }) {
  const brand = brandFor(agent);
  return (
    <div
      className="monogram"
      style={{
        width: size,
        height: size,
        borderRadius: Math.round(size * 0.34),
        background: `linear-gradient(145deg, ${brand.bg}, ${brand.bg}88)`,
        color: brand.color,
        fontSize: Math.round(size * 0.42),
      }}
    >
      {brand.letter}
    </div>
  );
}

export function StatusDot({
  status,
  size = 5,
}: {
  status: string;
  size?: number;
}) {
  return (
    <i
      className={`status-dot ${status}`}
      style={{ width: size, height: size }}
    />
  );
}

export function Eq({ playing }: { playing: boolean }) {
  return (
    <span className={`eq ${playing ? "on" : ""}`} aria-hidden>
      <i />
      <i />
      <i />
    </span>
  );
}

export function MiniRing({
  progress,
  color,
  size = 18,
}: {
  progress: number;
  color: string;
  size?: number;
}) {
  return (
    <span
      className="mini-ring"
      style={{
        width: size,
        height: size,
        background: `conic-gradient(${color} ${Math.round(
          Math.min(1, Math.max(0, progress)) * 360,
        )}deg, rgba(255,255,255,.13) 0)`,
      }}
    />
  );
}

export function Ring({
  progress,
  size = 108,
  stroke = 5,
  color,
  children,
}: {
  progress: number;
  size?: number;
  stroke?: number;
  color: string;
  children: ReactNode;
}) {
  const radius = (size - stroke) / 2;
  const circumference = 2 * Math.PI * radius;
  const offset = circumference * (1 - Math.min(1, Math.max(0, progress)));
  return (
    <div className="ring" style={{ width: size, height: size }}>
      <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`}>
        <circle
          cx={size / 2}
          cy={size / 2}
          r={radius}
          fill="none"
          stroke="rgba(255,255,255,.08)"
          strokeWidth={stroke}
        />
        <circle
          cx={size / 2}
          cy={size / 2}
          r={radius}
          fill="none"
          stroke={color}
          strokeWidth={stroke}
          strokeLinecap="round"
          strokeDasharray={circumference}
          strokeDashoffset={offset}
          transform={`rotate(-90 ${size / 2} ${size / 2})`}
          style={{ transition: "stroke-dashoffset .9s linear" }}
        />
      </svg>
      <div className="ring-inner">{children}</div>
    </div>
  );
}

export function Toggle({
  on,
  onChange,
  label,
}: {
  on: boolean;
  onChange: (next: boolean) => void;
  label: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={label}
      className={`toggle ${on ? "on" : ""}`}
      onClick={() => onChange(!on)}
    >
      <span />
    </button>
  );
}

export function CheckBubble({ done }: { done: boolean }) {
  return (
    <span className={`task-check ${done ? "done" : ""}`}>
      {done && <Check size={11} strokeWidth={3.2} />}
    </span>
  );
}
