export type Tab =
  | "media"
  | "timer"
  | "tasks"
  | "notes"
  | "calendar"
  | "agents"
  | "settings";

export type AgentStatus =
  | "working"
  | "thinking"
  | "waiting"
  | "done"
  | "error"
  | "idle";

export interface MediaSnapshot {
  active: boolean;
  title: string;
  artist: string;
  album: string;
  source: string;
  playing: boolean;
  status: string;
  positionMs: number;
  durationMs: number;
  trackKey: string;
  receivedAt?: number;
}

export interface AgentSession {
  id: string;
  agent: string;
  title: string;
  status: AgentStatus;
  task: string;
  tool: string;
  detail: string;
  progress: number | null;
  cwd: string;
  model: string;
  startedAt: number;
  lastAt: number;
  completedAt: number | null;
  step: number;
  lastMessage: string;
  /** Set while a permission request is blocked waiting for an answer. */
  requestId?: string;
}

export interface RunningSource {
  id: string;
  count: number;
}

export interface Task {
  id: string;
  text: string;
  done: boolean;
  createdAt: number;
  doneAt?: number;
}

export interface Note {
  id: string;
  text: string;
  createdAt: number;
  updatedAt: number;
}

export interface TimerState {
  durationMs: number;
  endsAt: number | null;
  remainingMs: number;
  running: boolean;
  finishedAt: number | null;
}

export interface Settings {
  expandOnHover: boolean;
  launchAtLogin: boolean;
  showClock: boolean;
  hideFullscreen: boolean;
  translucent: boolean;
  calendarUrl: string;
  calendarRemind: number;
}

export interface CalendarEvent {
  id: string;
  title: string;
  start: number;
  end: number;
  allDay: boolean;
  location: string;
  calendar: string;
}

export interface CalendarSnapshot {
  source: string;
  events: CalendarEvent[];
  updatedAt: number | null;
  error: string | null;
}

export interface WindowPos {
  x: number;
  y: number;
}

export interface PersistedState {
  tasks: Task[];
  notes?: Note[];
  timer: TimerState;
  settings: Settings;
  windowPos?: WindowPos | null;
}

export interface AppInfo {
  version: string;
  exe: string;
  dataDir: string;
  port: number;
}

export interface AdapterStatus {
  id: string;
  installed: boolean;
  /** Installed hooks point at the currently running executable. */
  current: boolean;
  path: string;
}

export interface GoogleStatus {
  connected: boolean;
  error: string | null;
}

export interface Peek {
  kind: "timer" | "agent" | "media" | "calendar";
  title: string;
  subtitle: string;
  tab: Tab;
}
