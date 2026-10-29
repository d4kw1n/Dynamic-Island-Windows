import { useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Island } from "./components/Island";
import { chime, primeAudio } from "./lib/audio";
import { brandFor } from "./lib/agents";
import { invoke, isTauri, listen } from "./lib/bridge";
import { fmtClock } from "./lib/format";
import { useStore } from "./store/useStore";
import type {
  AgentSession,
  AppInfo,
  CalendarSnapshot,
  MediaSnapshot,
  PersistedState,
  RunningSource,
} from "./types";

export default function App() {
  useHydration();
  useEvents();
  usePersistence();
  useTimerEngine();
  useWindowPlacement();
  useWindowFit();
  useFullscreenWatch();
  useCalendarSync();
  useCalendarReminders();
  usePreviewSeed();

  useEffect(() => {
    const onGesture = () => primeAudio();
    window.addEventListener("pointerdown", onGesture, { once: true });
    window.addEventListener("keydown", onGesture, { once: true });
    return () => {
      window.removeEventListener("pointerdown", onGesture);
      window.removeEventListener("keydown", onGesture);
    };
  }, []);

  return (
    <>
      {!isTauri && <div className="preview-desktop" aria-hidden />}
      <div className="stage">
        <Island />
      </div>
    </>
  );
}

function useHydration() {
  const hydrate = useStore((s) => s.hydrate);
  const setInfo = useStore((s) => s.setInfo);
  const setAgents = useStore((s) => s.setAgents);

  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const state = await invoke<Partial<PersistedState>>("load_state");
        if (alive) hydrate(state ?? {});
      } catch {
        if (alive) hydrate({});
      }
      try {
        const info = await invoke<AppInfo>("app_paths");
        if (alive && info) setInfo(info);
      } catch {
        /* preview */
      }
      try {
        const agents = await invoke<AgentSession[]>("agents_snapshot");
        if (alive && Array.isArray(agents) && agents.length > 0) {
          setAgents(agents);
        }
      } catch {
        /* preview */
      }
    })();
    return () => {
      alive = false;
    };
  }, [hydrate, setInfo, setAgents]);
}

function useEvents() {
  useEffect(() => {
    let alive = true;
    const unlisteners: Array<() => void> = [];
    const add = (fn: () => void) => {
      if (alive) unlisteners.push(fn);
      else fn();
    };

    void listen<MediaSnapshot>("media://update", (payload) => {
      useStore.getState().setMedia(payload);
    }).then(add);

    void listen<{ key: string; data: string }>(
      "media://artwork",
      (payload) => {
        useStore.getState().setArtwork(payload.key, payload.data);
      },
    ).then(add);

    void listen<AgentSession[]>("agents://update", (payload) => {
      const state = useStore.getState();
      const previous = state.agents;
      state.setAgents(Array.isArray(payload) ? payload : []);

      for (const session of payload) {
        const before = previous.find((item) => item.id === session.id);
        if (!before) continue;
        const finished =
          (session.status === "done" || session.status === "error") &&
          (before.status === "working" ||
            before.status === "waiting" ||
            before.status === "thinking");
        if (finished) {
          state.setPeek({
            kind: "agent",
            title: `${brandFor(session.agent).name} ${
              session.status === "error" ? "hit an error" : "finished"
            }`,
            subtitle:
              session.lastMessage || session.task || session.title || "Done",
            tab: "agents",
          });
          break;
        }
      }
    }).then(add);

    void listen<RunningSource[]>("agents://sources", (payload) => {
      useStore.getState().setSources(Array.isArray(payload) ? payload : []);
    }).then(add);

    void listen<string>("agents://error", (message) => {
      useStore.getState().showToast(`Agent webhook: ${message}`);
    }).then(add);

    return () => {
      alive = false;
      for (const unlisten of unlisteners) unlisten();
    };
  }, []);
}

function usePersistence() {
  const hydrated = useStore((s) => s.hydrated);

  useEffect(() => {
    if (!hydrated) return;
    let timer: number | undefined;

    const unsubscribe = useStore.subscribe((state, previous) => {
      if (
        state.tasks === previous.tasks &&
        state.notes === previous.notes &&
        state.timer === previous.timer &&
        state.settings === previous.settings &&
        state.windowPos === previous.windowPos
      ) {
        return;
      }
      if (timer) window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        void invoke("save_state", {
          data: {
            tasks: state.tasks,
            notes: state.notes,
            timer: state.timer,
            settings: state.settings,
            windowPos: state.windowPos,
          },
        }).catch(() => {});
      }, 280);
    });

    return () => {
      unsubscribe();
      if (timer) window.clearTimeout(timer);
    };
  }, [hydrated]);
}

/** Restores the saved island position and remembers native drags. */
function useWindowPlacement() {
  const hydrated = useStore((s) => s.hydrated);

  useEffect(() => {
    if (!isTauri || !hydrated) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    let saveTimer: number | undefined;

    void (async () => {
      const win = getCurrentWindow();
      const saved = useStore.getState().windowPos;
      if (saved) {
        await invoke("set_window_pos", { x: saved.x, y: saved.y }).catch(
          () => {},
        );
      }
      const stop = await win
        .onMoved(({ payload }) => {
          if (disposed) return;
          if (saveTimer) window.clearTimeout(saveTimer);
          saveTimer = window.setTimeout(() => {
            useStore
              .getState()
              .setWindowPos({ x: Math.round(payload.x), y: Math.round(payload.y) });
          }, 420);
        })
        .catch(() => undefined);
      if (stop) unlisten = stop;
      else if (!disposed) unlisten = () => {};
    })();

    return () => {
      disposed = true;
      if (saveTimer) window.clearTimeout(saveTimer);
      unlisten?.();
    };
  }, [hydrated]);
}

/**
 * Keeps the CSS viewport at least as large as the panel design (560x440).
 * WebView2 can map the window to fewer CSS pixels at some display scales,
 * which clipped panels and pushed buttons outside the native hit region.
 */
function useWindowFit() {
  useEffect(() => {
    if (!isTauri) return;
    let cancelled = false;
    let tries = 0;

    const fit = async () => {
      if (cancelled || tries >= 4) return;
      const width = window.innerWidth;
      const height = window.innerHeight;
      if (width >= 560 && height >= 440) return;
      tries += 1;
      try {
        const grown = await invoke<boolean>("fit_island_window", {
          cssWidth: width,
          cssHeight: height,
        });
        if (grown && !cancelled) window.setTimeout(fit, 140);
      } catch {
        /* keep the current size */
      }
    };

    const id = window.setTimeout(fit, 80);
    return () => {
      cancelled = true;
      window.clearTimeout(id);
    };
  }, []);
}

/** Hides the island while another app owns a true fullscreen surface. */
function useFullscreenWatch() {
  useEffect(() => {
    if (!isTauri) return;
    let unlisten: (() => void) | undefined;
    void listen<boolean>("island://fullscreen", (value) => {
      useStore.getState().setFullscreen(Boolean(value));
    }).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, []);
}

/** Pushes the saved feed URL to Rust and receives parsed events. */
function useCalendarSync() {
  const hydrated = useStore((s) => s.hydrated);
  const calendarUrl = useStore((s) => s.settings.calendarUrl);

  useEffect(() => {
    if (!isTauri || !hydrated) return;
    void invoke("calendar_set_source", { source: calendarUrl }).catch(() => {});
  }, [hydrated, calendarUrl]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void listen<CalendarSnapshot>("calendar://update", (snapshot) => {
      if (snapshot && Array.isArray(snapshot.events)) {
        useStore.getState().setCalendar(snapshot);
      }
    }).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, []);
}

const remindedEvents = new Map<string, number>();

/** Reminds once per event through the same peek + notification path as timers. */
function useCalendarReminders() {
  const events = useStore((s) => s.calendar.events);
  const lead = useStore((s) => s.settings.calendarRemind);

  useEffect(() => {
    if (lead <= 0) return;

    const tick = () => {
      const state = useStore.getState();
      const now = Date.now();
      const horizon = lead * 60_000;

      for (const event of state.calendar.events) {
        if (event.allDay) continue;
        const delta = event.start - now;
        if (delta < 0 || delta > horizon) continue;
        if (remindedEvents.has(event.id)) continue;

        remindedEvents.set(event.id, now);
        const when = fmtClock(event.start);
        const minutes = Math.max(1, Math.round(delta / 60_000));
        state.setPeek({
          kind: "calendar",
          title: event.title,
          subtitle: `${when} · starts in ${minutes} min`,
          tab: "calendar",
        });
        state.showToast(`${event.title} at ${when}`);
        chime();
        void invoke("notify", {
          title: `${event.title} at ${when}`,
          body: event.location || "Calendar reminder",
        }).catch(() => {});
        break;
      }

      if (remindedEvents.size > 300) {
        const cutoff = now - 86_400_000;
        for (const [id, at] of remindedEvents) {
          if (at < cutoff) remindedEvents.delete(id);
        }
      }
    };

    tick();
    const id = window.setInterval(tick, 15_000);
    return () => window.clearInterval(id);
  }, [events, lead]);
}

/** Browser preview helper: ?seed=timer gives the screenshot suite something
 *  to render in the multi-activity cluster. */
function usePreviewSeed() {
  const hydrated = useStore((s) => s.hydrated);
  useEffect(() => {
    if (isTauri || !hydrated) return;
    const seed = new URLSearchParams(window.location.search).get("seed");
    if (seed?.includes("timer")) {
      useStore.getState().startTimer(22 * 60_000);
    }
  }, [hydrated]);
}

function useTimerEngine() {
  useEffect(() => {
    const id = window.setInterval(() => {
      const state = useStore.getState();
      const { timer } = state;
      if (!timer.running || !timer.endsAt) return;
      if (Date.now() < timer.endsAt) return;

      state.finishTimer();
      state.setPeek({
        kind: "timer",
        title: "Timer finished",
        subtitle: "Your focus session is complete",
        tab: "timer",
      });
      state.showToast("Timer finished");
      chime();
      void invoke("notify", {
        title: "Timer finished",
        body: "Your focus session is complete.",
      }).catch(() => {});
    }, 250);

    return () => window.clearInterval(id);
  }, []);
}
