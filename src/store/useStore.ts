import { create } from "zustand";
import type {
  AgentSession,
  AppInfo,
  CalendarSnapshot,
  MediaSnapshot,
  Note,
  Peek,
  PersistedState,
  RunningSource,
  Settings,
  Task,
  TimerState,
  WindowPos,
} from "../types";

export const DEFAULT_TIMER: TimerState = {
  durationMs: 25 * 60_000,
  endsAt: null,
  remainingMs: 25 * 60_000,
  running: false,
  finishedAt: null,
};

export const DEFAULT_SETTINGS: Settings = {
  expandOnHover: true,
  launchAtLogin: false,
  showClock: true,
  hideFullscreen: true,
  translucent: true,
  calendarUrl: "",
  calendarRemind: 10,
};

export function timerRemaining(timer: TimerState, now: number): number {
  if (timer.running && timer.endsAt) {
    return Math.max(0, timer.endsAt - now);
  }
  return Math.max(0, timer.remainingMs);
}

export function timerProgress(timer: TimerState, now: number): number {
  const remaining = timerRemaining(timer, now);
  if (timer.durationMs <= 0) return 0;
  return Math.min(1, Math.max(0, 1 - remaining / timer.durationMs));
}

export function isTimerVisible(timer: TimerState, now: number): boolean {
  if (timer.running) return true;
  if (timer.remainingMs > 0 && timer.remainingMs < timer.durationMs) return true;
  return timer.finishedAt !== null && now - timer.finishedAt < 90_000;
}

function uid(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

interface AppStore {
  hydrated: boolean;
  tasks: Task[];
  notes: Note[];
  timer: TimerState;
  settings: Settings;
  media: MediaSnapshot;
  artwork: Record<string, string>;
  calendar: CalendarSnapshot;
  agents: AgentSession[];
  sources: RunningSource[];
  info: AppInfo | null;
  toast: string | null;
  peek: Peek | null;
  windowPos: WindowPos | null;
  fullscreen: boolean;

  hydrate: (state: Partial<PersistedState>) => void;
  setInfo: (info: AppInfo) => void;
  setWindowPos: (pos: WindowPos | null) => void;
  setFullscreen: (value: boolean) => void;

  addTask: (text: string) => void;
  toggleTask: (id: string) => void;
  removeTask: (id: string) => void;
  clearDone: () => void;

  addNote: (text: string) => void;
  updateNote: (id: string, text: string) => void;
  removeNote: (id: string) => void;

  startTimer: (durationMs: number) => void;
  pauseTimer: () => void;
  resumeTimer: () => void;
  resetTimer: () => void;
  finishTimer: () => void;

  updateSettings: (patch: Partial<Settings>) => void;
  setMedia: (media: MediaSnapshot) => void;
  setArtwork: (key: string, data: string) => void;
  setCalendar: (calendar: CalendarSnapshot) => void;
  setAgents: (agents: AgentSession[]) => void;
  setSources: (sources: RunningSource[]) => void;
  showToast: (message: string) => void;
  setPeek: (peek: Peek | null) => void;
}

let toastTimer: number | undefined;

/**
 * Playback clock for the media timeline. SMTC often reports positions in
 * coarse steps and the poll re-sends the same value; re-anchoring on every
 * snapshot made the seek bar creep forward and snap back. We keep our own
 * anchor and only jump when the player seeks.
 */
let mediaClock = { key: "", lastRaw: 0, lastRawAt: 0, anchor: 0, at: 0 };

function anchoredMedia(media: MediaSnapshot): MediaSnapshot {
  const now = Date.now();
  const key = media.active ? media.trackKey : "";

  if (key !== mediaClock.key || !media.playing) {
    mediaClock = {
      key,
      lastRaw: media.positionMs,
      lastRawAt: now,
      anchor: media.positionMs,
      at: now,
    };
    return { ...media, receivedAt: now };
  }

  const raw = media.positionMs;
  // Where the last raw report should be by now, if playback simply continued.
  const expectedRaw = mediaClock.lastRaw + (now - mediaClock.lastRawAt);
  const seekBack = raw + 500 < mediaClock.lastRaw;
  const seekAhead = raw > expectedRaw + 1_000;
  if (seekBack || seekAhead) {
    mediaClock = { key, lastRaw: raw, lastRawAt: now, anchor: raw, at: now };
  } else if (raw >= mediaClock.lastRaw) {
    mediaClock.lastRaw = raw;
    mediaClock.lastRawAt = now;
  }

  const position = Math.min(
    mediaClock.anchor + (now - mediaClock.at),
    media.durationMs > 0 ? media.durationMs : Number.MAX_SAFE_INTEGER,
  );
  return { ...media, positionMs: Math.max(0, position), receivedAt: now };
}

export const useStore = create<AppStore>()((set) => ({
  hydrated: false,
  tasks: [],
  notes: [],
  timer: DEFAULT_TIMER,
  settings: DEFAULT_SETTINGS,
  media: {
    active: false,
    title: "",
    artist: "",
    album: "",
    source: "",
    playing: false,
    status: "stopped",
    positionMs: 0,
    durationMs: 0,
    trackKey: "",
  },
  artwork: {},
  calendar: {
    source: "",
    events: [],
    updatedAt: null,
    error: null,
  },
  agents: [],
  sources: [],
  info: null,
  toast: null,
  peek: null,
  windowPos: null,
  fullscreen: false,

  hydrate: (state) => {
    const settings: Settings = { ...DEFAULT_SETTINGS, ...(state.settings ?? {}) };
    let timer: TimerState = { ...DEFAULT_TIMER, ...(state.timer ?? {}) };
    if (timer.endsAt && timer.endsAt <= Date.now()) {
      timer = {
        ...timer,
        running: false,
        endsAt: null,
        remainingMs: 0,
        finishedAt: null,
      };
    }
    set({
      tasks: Array.isArray(state.tasks) ? state.tasks : [],
      notes: Array.isArray(state.notes)
        ? state.notes.filter((note) => note && typeof note.text === "string")
        : [],
      timer,
      settings,
      windowPos:
        state.windowPos &&
        Number.isFinite(state.windowPos.x) &&
        Number.isFinite(state.windowPos.y)
          ? state.windowPos
          : null,
      hydrated: true,
    });
  },

  setInfo: (info) => set({ info }),

  setWindowPos: (windowPos) => set({ windowPos }),

  setFullscreen: (fullscreen) => set({ fullscreen }),

  addTask: (text) => {
    const value = text.trim();
    if (!value) return;
    const task: Task = {
      id: uid(),
      text: value,
      done: false,
      createdAt: Date.now(),
    };
    set((state) => ({ tasks: [task, ...state.tasks] }));
  },

  toggleTask: (id) =>
    set((state) => ({
      tasks: state.tasks.map((task) =>
        task.id === id
          ? { ...task, done: !task.done, doneAt: !task.done ? Date.now() : undefined }
          : task,
      ),
    })),

  removeTask: (id) =>
    set((state) => ({ tasks: state.tasks.filter((task) => task.id !== id) })),

  clearDone: () => set((state) => ({ tasks: state.tasks.filter((t) => !t.done) })),

  addNote: (text) => {
    const value = text.trim();
    if (!value) return;
    const now = Date.now();
    const note: Note = {
      id: uid(),
      text: value,
      createdAt: now,
      updatedAt: now,
    };
    set((state) => ({ notes: [note, ...state.notes] }));
  },

  updateNote: (id, text) =>
    set((state) => {
      const value = text.trim();
      if (!value) return { notes: state.notes.filter((note) => note.id !== id) };
      return {
        notes: state.notes.map((note) =>
          note.id === id
            ? { ...note, text: value, updatedAt: Date.now() }
            : note,
        ),
      };
    }),

  removeNote: (id) =>
    set((state) => ({ notes: state.notes.filter((note) => note.id !== id) })),

  startTimer: (durationMs) => {
    const duration = Math.max(1_000, Math.round(durationMs));
    set({
      timer: {
        durationMs: duration,
        endsAt: Date.now() + duration,
        remainingMs: duration,
        running: true,
        finishedAt: null,
      },
    });
  },

  pauseTimer: () =>
    set((state) => {
      const { timer } = state;
      if (!timer.running || !timer.endsAt) return {};
      const remaining = Math.max(0, timer.endsAt - Date.now());
      return {
        timer: {
          ...timer,
          running: false,
          endsAt: null,
          remainingMs: remaining,
        },
      };
    }),

  resumeTimer: () =>
    set((state) => {
      const { timer } = state;
      if (timer.running) return {};
      const remaining = timer.remainingMs > 0 ? timer.remainingMs : timer.durationMs;
      return {
        timer: {
          ...timer,
          running: true,
          endsAt: Date.now() + remaining,
          remainingMs: remaining,
          finishedAt: null,
        },
      };
    }),

  resetTimer: () =>
    set((state) => ({
      timer: {
        ...state.timer,
        running: false,
        endsAt: null,
        remainingMs: state.timer.durationMs,
        finishedAt: null,
      },
    })),

  finishTimer: () =>
    set((state) => ({
      timer: {
        ...state.timer,
        running: false,
        endsAt: null,
        remainingMs: 0,
        finishedAt: Date.now(),
      },
    })),

  updateSettings: (patch) =>
    set((state) => ({ settings: { ...state.settings, ...patch } })),

  setMedia: (media) => set({ media: anchoredMedia(media) }),

  setArtwork: (key, data) =>
    set((state) => {
      const artwork = { ...state.artwork, [key]: data };
      const keys = Object.keys(artwork);
      if (keys.length > 4) {
        for (const old of keys.slice(0, keys.length - 4)) {
          delete artwork[old];
        }
      }
      return { artwork };
    }),

  setCalendar: (calendar) =>
    set({
      calendar: {
        ...calendar,
        events: Array.isArray(calendar.events) ? calendar.events : [],
      },
    }),

  setAgents: (agents) => set({ agents }),

  setSources: (sources) => set({ sources }),

  showToast: (message) => {
    if (toastTimer) window.clearTimeout(toastTimer);
    set({ toast: message });
    toastTimer = window.setTimeout(() => set({ toast: null }), 3400);
  },

  setPeek: (peek) => set({ peek }),
}));
