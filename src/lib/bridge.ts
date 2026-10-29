import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AdapterStatus,
  AgentSession,
  AppInfo,
  CalendarEvent,
  CalendarSnapshot,
  GoogleStatus,
  MediaSnapshot,
  PersistedState,
  RunningSource,
  Settings,
} from "../types";

export const isTauri =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

type Handler<T> = (payload: T) => void;

const mockHandlers = new Map<string, Set<Handler<never>>>();

function mockEmit<T>(event: string, payload: T) {
  const handlers = mockHandlers.get(event);
  if (!handlers) return;
  for (const handler of handlers) {
    (handler as Handler<T>)(payload);
  }
}

function mockListen<T>(event: string, handler: Handler<T>): UnlistenFn {
  let handlers = mockHandlers.get(event);
  if (!handlers) {
    handlers = new Set();
    mockHandlers.set(event, handlers);
  }
  handlers.add(handler as Handler<never>);
  return () => {
    handlers?.delete(handler as Handler<never>);
  };
}

export async function invoke<T>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (isTauri) {
    return tauriInvoke<T>(cmd, args);
  }
  return mockInvoke(cmd, args) as T;
}

export async function listen<T>(
  event: string,
  handler: Handler<T>,
): Promise<UnlistenFn> {
  if (isTauri) {
    return tauriListen<T>(event, (payload) => handler(payload.payload));
  }
  return mockListen(event, handler);
}

// ---------------------------------------------------------------------------
// Browser preview mocks
// ---------------------------------------------------------------------------

const STORAGE_KEY = "island-preview-state-v1";
const AUTOSTART_KEY = "island-preview-autostart";
const PREVIEW_FEED =
  "https://calendar.google.com/calendar/ical/preview/basic.ics";

type PreviewState = Partial<Omit<PersistedState, "settings">> & {
  settings?: Partial<Settings>;
};

function readPreviewState(): Partial<PersistedState> {
  try {
    const raw = JSON.parse(
      localStorage.getItem(STORAGE_KEY) || "{}",
    ) as PreviewState;
    raw.settings = {
      calendarUrl: PREVIEW_FEED,
      calendarRemind: 10,
      ...(raw.settings ?? {}),
    };
    return raw as Partial<PersistedState>;
  } catch {
    return {};
  }
}

const MOCK_ART =
  "data:image/svg+xml;utf8," +
  encodeURIComponent(
    `<svg xmlns='http://www.w3.org/2000/svg' width='320' height='320' viewBox='0 0 320 320'>
      <defs>
        <linearGradient id='a' x1='0' y1='0' x2='1' y2='1'>
          <stop offset='0' stop-color='#ff6b8a'/>
          <stop offset='.52' stop-color='#b44bff'/>
          <stop offset='1' stop-color='#2d6bff'/>
        </linearGradient>
        <radialGradient id='b' cx='.3' cy='.22' r='.8'>
          <stop offset='0' stop-color='#ffffff' stop-opacity='.5'/>
          <stop offset='1' stop-color='#ffffff' stop-opacity='0'/>
        </radialGradient>
      </defs>
      <rect width='320' height='320' fill='#0b0c10'/>
      <circle cx='160' cy='160' r='104' fill='url(#a)'/>
      <circle cx='160' cy='160' r='104' fill='url(#b)'/>
      <circle cx='160' cy='160' r='30' fill='#08090c'/>
      <circle cx='160' cy='160' r='6' fill='#ff6b8a'/>
      <path d='M160 20v280M20 160h280' stroke='#ffffff' stroke-opacity='.10' stroke-width='1'/>
    </svg>`,
  );

let mockMedia: MediaSnapshot = {
  active: true,
  title: "Midnight City",
  artist: "M83",
  album: "Hurry Up, We're Dreaming",
  source: "Spotify.exe",
  playing: true,
  status: "playing",
  positionMs: 74_000,
  durationMs: 244_000,
  trackKey: "mock:midnight-city",
  receivedAt: Date.now(),
};

function seedAgents(): AgentSession[] {
  const now = Date.now();
  return [
    {
      id: "mock:zcode",
      agent: "zcode",
      title: "dynamic-island-windows",
      status: "working",
      task: "Build the island shell and design system",
      tool: "Edit",
      detail: "src/components/Island.tsx",
      progress: null,
      cwd: "D:\\Dev\\dynamic-island-windows",
      model: "GLM-5",
      startedAt: now - 226_000,
      lastAt: now - 2_000,
      completedAt: null,
      step: 14,
      lastMessage: "",
    },
    {
      id: "mock:claude",
      agent: "claude",
      title: "api-gateway",
      status: "waiting",
      task: "Refactor websocket reconnection",
      tool: "Bash",
      detail: "psql migration",
      progress: 0.62,
      cwd: "D:\\Dev\\api-gateway",
      model: "Claude Opus 4.5",
      startedAt: now - 512_000,
      lastAt: now - 18_000,
      completedAt: null,
      step: 31,
      lastMessage: "Permission required: apply database migration",
    },
    {
      id: "mock:codex",
      agent: "codex",
      title: "web-dashboard",
      status: "done",
      task: "Fix flaky integration tests",
      tool: "Bash",
      detail: "pnpm test",
      progress: 1,
      cwd: "D:\\Dev\\web-dashboard",
      model: "GPT-5.2-Codex",
      startedAt: now - 780_000,
      lastAt: now - 140_000,
      completedAt: now - 60_000,
      step: 22,
      lastMessage: "All 42 tests passed",
    },
  ];
}

let mockAgents = seedAgents();

function seedCalendar(): CalendarEvent[] {
  const at = (dayOffset: number, hour: number, minute: number) => {
    const date = new Date();
    date.setDate(date.getDate() + dayOffset);
    date.setHours(hour, minute, 0, 0);
    return date.getTime();
  };
  const inMinutes = (minutes: number) => Date.now() + minutes * 60_000;

  return [
    {
      id: "mock:design",
      title: "Design review",
      start: inMinutes(-10),
      end: inMinutes(50),
      allDay: false,
      location: "Studio · Meet",
      calendar: "Work",
    },
    {
      id: "mock:sprint",
      title: "Sprint planning",
      start: inMinutes(35),
      end: inMinutes(80),
      allDay: false,
      location: "Room 3",
      calendar: "Work",
    },
    {
      id: "mock:lunch",
      title: "Lunch with Linh",
      start: inMinutes(190),
      end: inMinutes(250),
      allDay: false,
      location: "The Deck",
      calendar: "Personal",
    },
    {
      id: "mock:sync",
      title: "Weekly sync",
      start: at(1, 9, 30),
      end: at(1, 10, 0),
      allDay: false,
      location: "",
      calendar: "Work",
    },
    {
      id: "mock:ship",
      title: "Ship v0.2",
      start: at(2, 0, 0),
      end: at(3, 0, 0),
      allDay: true,
      location: "",
      calendar: "Work",
    },
    {
      id: "mock:1on1",
      title: "1:1 with Minh",
      start: at(3, 14, 0),
      end: at(3, 14, 30),
      allDay: false,
      location: "",
      calendar: "Work",
    },
  ].sort((a, b) => a.start - b.start);
}

let mockCalendar: CalendarSnapshot = {
  source: PREVIEW_FEED,
  events: seedCalendar(),
  updatedAt: Date.now(),
  error: null,
};

async function mockDemo() {
  mockAgents = seedAgents().map((agent) =>
    agent.id === "mock:codex"
      ? { ...agent, status: "done", completedAt: Date.now(), progress: 1 }
      : agent,
  );
  mockEmit("agents://update", mockAgents);

  window.setTimeout(() => {
    mockAgents = mockAgents.map((agent) =>
      agent.id === "mock:zcode"
        ? {
            ...agent,
            tool: "Write",
            detail: "src-tauri/src/media.rs",
            lastAt: Date.now(),
            step: agent.step + 1,
          }
        : agent,
    );
    mockEmit("agents://update", mockAgents);
  }, 1600);

  window.setTimeout(() => {
    mockAgents = mockAgents.map((agent) =>
      agent.id === "mock:zcode"
        ? {
            ...agent,
            status: "done",
            progress: 1,
            completedAt: Date.now(),
            lastMessage: "Media integration complete",
          }
        : agent,
    );
    mockEmit("agents://update", mockAgents);
  }, 3600);
}

function mockInvoke(cmd: string, args?: Record<string, unknown>): unknown {
  const payload = (args ?? {}) as Record<string, unknown>;

  switch (cmd) {
    case "load_state":
      return readPreviewState();
    case "save_state":
      localStorage.setItem(STORAGE_KEY, JSON.stringify(payload.data ?? {}));
      return null;
    case "app_paths":
      return {
        version: "0.1.0",
        exe: "browser preview",
        dataDir: "",
        port: 17321,
      } satisfies AppInfo;
    case "set_hit_rect":
      return null;
    case "media_control": {
      const action = String(payload.action ?? "");
      if (action === "play_pause") {
        mockMedia = {
          ...mockMedia,
          playing: !mockMedia.playing,
          status: mockMedia.playing ? "paused" : "playing",
        };
      } else if (action === "next" || action === "previous") {
        mockMedia = {
          ...mockMedia,
          title: action === "next" ? "Outro" : "Intro",
          trackKey: action === "next" ? "mock:outro" : "mock:intro",
          positionMs: 0,
          durationMs: 214_000,
        };
        mockEmit("media://artwork", {
          key: mockMedia.trackKey,
          data: MOCK_ART,
        });
      } else if (action === "seek") {
        mockMedia = {
          ...mockMedia,
          positionMs: Number(payload.positionMs ?? 0),
        };
      }
      mockMedia.receivedAt = Date.now();
      mockEmit("media://update", mockMedia);
      return null;
    }
    case "adapter_status":
      return [
        {
          id: "claude",
          installed: false,
          current: false,
          path: "C:\\Users\\you\\.claude\\settings.json",
        },
        {
          id: "zcode",
          installed: true,
          current: true,
          path: "C:\\Users\\you\\.zcode\\cli\\config.json",
        },
        {
          id: "codex",
          installed: false,
          current: false,
          path: "C:\\Users\\you\\.codex\\config.toml",
        },
      ] satisfies AdapterStatus[];
    case "install_adapter":
      throw new Error(
        "Agent hooks can only be installed from the desktop app.",
      );
    case "uninstall_adapter":
      throw new Error(
        "Agent hooks can only be changed from the desktop app.",
      );
    case "google_status":
      return { connected: false, error: null } satisfies GoogleStatus;
    case "google_connect":
      throw new Error(
        "Google Calendar sign-in only works in the desktop app.",
      );
    case "google_disconnect":
      return null;
    case "get_autostart":
      return localStorage.getItem(AUTOSTART_KEY) === "1";
    case "set_autostart":
      localStorage.setItem(AUTOSTART_KEY, payload.enabled ? "1" : "0");
      return null;
    case "notify":
      try {
        if ("Notification" in window && Notification.permission === "granted") {
          new Notification(String(payload.title ?? ""), {
            body: String(payload.body ?? ""),
          });
        }
      } catch {
        /* preview only */
      }
      return null;
    case "calendar_set_source": {
      const source = String(payload.source ?? "");
      mockCalendar = source
        ? { ...mockCalendar, source, events: seedCalendar(), updatedAt: Date.now(), error: null }
        : { source: "", events: [], updatedAt: null, error: null };
      mockEmit("calendar://update", mockCalendar);
      return null;
    }
    case "calendar_refresh":
      mockCalendar = {
        ...mockCalendar,
        events: seedCalendar(),
        updatedAt: Date.now(),
      };
      mockEmit("calendar://update", mockCalendar);
      return null;
    case "agents_snapshot":
      return mockAgents;
    case "clear_agents":
      mockAgents = [];
      mockEmit("agents://update", mockAgents);
      return null;
    case "run_demo":
      void mockDemo();
      return null;
    case "set_window_pos":
      return null;
    case "ensure_island_visible":
      return false;
    case "reset_window_position":
      return null;
    case "open_data_dir":
    case "quit_app":
      return null;
    default:
      return null;
  }
}

if (!isTauri) {
  window.setTimeout(() => {
    mockEmit("media://update", mockMedia);
    mockEmit("media://artwork", {
      key: mockMedia.trackKey,
      data: MOCK_ART,
    });
    mockEmit<RunningSource[]>("agents://sources", [
      { id: "zcode", count: 1 },
      { id: "antigravity", count: 1 },
    ]);
    mockEmit("agents://update", mockAgents);
    mockEmit("calendar://update", mockCalendar);
  }, 250);

  window.setInterval(() => {
    if (!mockMedia.active) return;
    if (mockMedia.playing) {
      mockMedia.positionMs = Math.min(
        mockMedia.durationMs,
        mockMedia.positionMs + 1000,
      );
    }
    mockMedia.receivedAt = Date.now();
    mockEmit("media://update", mockMedia);
  }, 1000);
}
