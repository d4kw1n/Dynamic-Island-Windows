import { useEffect, useRef, useState } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";
import {
  CalendarDays,
  ChevronUp,
  GripVertical,
  Music2,
} from "lucide-react";
import { useNow } from "../hooks/useNow";
import { invoke, isTauri, listen } from "../lib/bridge";
import {
  currentEvent,
  imminentEvent,
  upcomingEvent,
} from "../lib/calendar";
import { fmtClock, fmtDuration, sourceLabel } from "../lib/format";
import { TAB_META, TAB_ORDER } from "../lib/tabs";
import {
  isTimerVisible,
  timerProgress,
  timerRemaining,
  useStore,
} from "../store/useStore";
import type { Tab } from "../types";
import { AgentsPanel } from "./AgentsPanel";
import { Eq, MiniRing, Monogram, StatusDot } from "./Bits";
import { CalendarPanel } from "./CalendarPanel";
import { MediaPanel } from "./MediaPanel";
import { NotesPanel } from "./NotesPanel";
import { SettingsPanel } from "./SettingsPanel";
import { TasksPanel } from "./TasksPanel";
import { TimerPanel } from "./TimerPanel";

type CollapsedKind =
  | "idle"
  | "media"
  | "timer"
  | "calendar"
  | "agents"
  | "peek"
  | "multi";

const COLLAPSED: Record<CollapsedKind, { w: number; h: number; r: number }> = {
  idle: { w: 138, h: 34, r: 17 },
  media: { w: 312, h: 36, r: 18 },
  timer: { w: 228, h: 36, r: 18 },
  calendar: { w: 258, h: 36, r: 18 },
  agents: { w: 300, h: 36, r: 18 },
  peek: { w: 370, h: 48, r: 24 },
  multi: { w: 328, h: 38, r: 19 },
};

const SEG_W = { media: 148, timer: 90, calendar: 84, agents: 72 } as const;

const EXPANDED: Record<Tab, { w: number; h: number }> = {
  media: { w: 426, h: 238 },
  timer: { w: 412, h: 222 },
  tasks: { w: 428, h: 330 },
  notes: { w: 430, h: 340 },
  calendar: { w: 430, h: 366 },
  agents: { w: 474, h: 410 },
  settings: { w: 438, h: 396 },
};

export function Island() {
  const media = useStore((s) => s.media);
  const artworkMap = useStore((s) => s.artwork);
  const agents = useStore((s) => s.agents);
  const calendar = useStore((s) => s.calendar);
  const timer = useStore((s) => s.timer);
  const tasks = useStore((s) => s.tasks);
  const notes = useStore((s) => s.notes);
  const settings = useStore((s) => s.settings);
  const fullscreen = useStore((s) => s.fullscreen);
  const peek = useStore((s) => s.peek);
  const setPeek = useStore((s) => s.setPeek);
  const toast = useStore((s) => s.toast);
  const info = useStore((s) => s.info);

  const params =
    typeof window !== "undefined"
      ? new URLSearchParams(window.location.search)
      : null;
  const requestedTab = params?.get("tab") as Tab | null;

  const [expanded, setExpanded] = useState(
    () => params?.get("expanded") === "1",
  );
  const [tab, setTab] = useState<Tab>(() =>
    requestedTab && requestedTab in EXPANDED ? requestedTab : "media",
  );
  const [pinned, setPinned] = useState(false);
  const [dragging, setDragging] = useState(false);

  const rootRef = useRef<HTMLDivElement>(null);
  const hoverTimer = useRef<number | null>(null);
  const collapseTimer = useRef<number | null>(null);
  const hoverTab = useRef<Tab | null>(null);
  const interactive = useRef(false);
  const lastInteract = useRef(Date.now());
  const pinnedRef = useRef(false);
  const expandedRef = useRef(false);
  const concealedRef = useRef(false);
  const dragRef = useRef<{ x: number; y: number; active: boolean } | null>(null);
  const suppressClick = useRef(false);

  const now = useNow(1000);

  useEffect(() => {
    pinnedRef.current = pinned;
  }, [pinned]);
  useEffect(() => {
    expandedRef.current = expanded;
  }, [expanded]);

  const activeAgents = agents.filter(
    (agent) =>
      agent.status === "working" ||
      agent.status === "waiting" ||
      agent.status === "thinking",
  );
  const waitingAgents = agents.filter((agent) => agent.status === "waiting");
  const workingAgents = agents.filter((agent) => agent.status === "working");
  const openTasks = tasks.filter((task) => !task.done).length;
  const doneTasks = tasks.length - openTasks;

  const timerVisible = isTimerVisible(timer, now);
  const liveMedia = media.active;
  const liveTimer = timerVisible;
  const calendarNext = imminentEvent(calendar.events, now);
  const liveCalendar = Boolean(calendarNext);
  const liveAgents = activeAgents.length > 0;
  const liveCount =
    Number(liveMedia) +
    Number(liveTimer) +
    Number(liveCalendar) +
    Number(liveAgents);

  const priority: Exclude<CollapsedKind, "peek" | "multi"> = liveTimer
    ? "timer"
    : liveMedia
      ? "media"
      : liveCalendar
        ? "calendar"
        : liveAgents
          ? "agents"
          : "idle";
  const kind: CollapsedKind =
    peek && !expanded
      ? "peek"
      : liveCount >= 2
        ? "multi"
        : priority;
  const priorityTab: Tab =
    priority === "timer"
      ? "timer"
      : priority === "media"
        ? "media"
        : priority === "calendar"
          ? "calendar"
          : priority === "agents"
            ? "agents"
            : "tasks";

  const clusterWidth =
    16 +
    (liveMedia ? SEG_W.media : 0) +
    (liveTimer ? SEG_W.timer : 0) +
    (liveCalendar ? SEG_W.calendar : 0) +
    (liveAgents ? SEG_W.agents : 0) +
    Math.max(0, liveCount - 1);

  const dims = expanded
    ? { ...EXPANDED[tab], r: 28 }
    : kind === "multi"
      ? { w: clusterWidth, h: 38, r: 19 }
      : COLLAPSED[kind];

  const concealed = fullscreen && settings.hideFullscreen;
  const showLiveDots = liveCount >= 2;
  const liveTabs: Partial<Record<Tab, boolean>> = {
    media: liveMedia,
    timer: liveTimer,
    calendar: liveCalendar,
    agents: liveAgents,
  };

  const clearTimers = () => {
    if (hoverTimer.current) window.clearTimeout(hoverTimer.current);
    if (collapseTimer.current) window.clearTimeout(collapseTimer.current);
    hoverTimer.current = null;
    collapseTimer.current = null;
  };

  const open = (next: Tab) => {
    clearTimers();
    setTab(next);
    setExpanded(true);
    setPeek(null);
    lastInteract.current = Date.now();
  };

  const collapse = () => {
    clearTimers();
    hoverTab.current = null;
    setExpanded(false);
    setPinned(false);
  };

  // Fullscreen apps get the whole screen: tuck the island away entirely.
  useEffect(() => {
    concealedRef.current = concealed;
    if (!concealed) return;
    clearTimers();
    hoverTab.current = null;
    dragRef.current = null;
    setDragging(false);
    setExpanded(false);
    setPinned(false);
  }, [concealed]);

  useEffect(() => {
    return () => clearTimers();
  }, []);

  const startNativeDrag = () => {
    if (!isTauri) return;
    setDragging(true);
    void invoke("start_window_drag").catch(() => {});
    window.setTimeout(() => {
      suppressClick.current = false;
      dragRef.current = null;
      setDragging(false);
    }, 500);
  };

  const onRootPointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    lastInteract.current = Date.now();
    if (expandedRef.current) {
      setPinned(true);
      return;
    }
    if (!isTauri) return;
    const target = event.target as HTMLElement;
    if (target.closest("button, input, a") && !target.closest(".cluster-seg")) {
      return;
    }
    dragRef.current = { x: event.clientX, y: event.clientY, active: false };
  };

  const onRootPointerMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    lastInteract.current = Date.now();
    const drag = dragRef.current;
    if (!drag || drag.active || expandedRef.current) return;
    if (Math.hypot(event.clientX - drag.x, event.clientY - drag.y) < 7) return;
    drag.active = true;
    suppressClick.current = true;
    interactive.current = false;
    if (hoverTimer.current) {
      window.clearTimeout(hoverTimer.current);
      hoverTimer.current = null;
    }
    startNativeDrag();
  };

  const onRootPointerUp = () => {
    if (dragRef.current && !dragRef.current.active) dragRef.current = null;
  };

  // Peek auto-dismiss.
  useEffect(() => {
    if (!peek) return;
    const id = window.setTimeout(() => setPeek(null), 4600);
    return () => window.clearTimeout(id);
  }, [peek, setPeek]);

  // Escape collapses the panel.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") collapse();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Pinned panels still close after a long idle period.
  useEffect(() => {
    if (!expanded || !pinned) return;
    const id = window.setInterval(() => {
      const el = rootRef.current;
      if (el && el.contains(document.activeElement)) {
        lastInteract.current = Date.now();
        return;
      }
      // Never hide a panel that is waiting for a permission decision.
      if (useStore.getState().agents.some((agent) => Boolean(agent.requestId))) {
        lastInteract.current = Date.now();
        return;
      }
      if (Date.now() - lastInteract.current > 20_000) {
        collapse();
      }
    }, 1000);
    return () => window.clearInterval(id);
  }, [expanded, pinned]);

  // Hover-opened panels must close again when the pointer leaves. The webview
  // stops receiving mouse events the moment the window turns click-through at
  // the island edge (no `mouseleave` arrives), so ask the native cursor poll.
  useEffect(() => {
    if (!isTauri || !expanded || pinned) return;
    let outsideSince: number | null = null;
    const id = window.setInterval(() => {
      void invoke<boolean | null>("cursor_over_island")
        .then((inside) => {
          if (inside === null || inside === undefined) return;
          if (inside) {
            outsideSince = null;
            return;
          }
          if (outsideSince === null) {
            outsideSince = Date.now();
            return;
          }
          if (Date.now() - outsideSince > 650 && !pinnedRef.current) {
            outsideSince = null;
            collapse();
          }
        })
        .catch(() => {});
    }, 160);
    return () => window.clearInterval(id);
  }, [expanded, pinned]);

  // A permission request is time-sensitive: surface the agents panel and keep
  // it open until the question is answered.
  const seenRequests = useRef<Set<string>>(new Set());
  useEffect(() => {
    const request = agents.find(
      (agent) => agent.status === "waiting" && Boolean(agent.requestId),
    );
    const requestId = request?.requestId;
    if (!requestId || seenRequests.current.has(requestId)) return;
    seenRequests.current.add(requestId);
    if (concealedRef.current) return;
    clearTimers();
    setTab("agents");
    setExpanded(true);
    setPinned(true);
    setPeek(null);
    lastInteract.current = Date.now();
  }, [agents]);

  // Tray -> settings.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void listen<void>("island://open-settings", () => {
      setTab("settings");
      setExpanded(true);
      setPinned(true);
      setPeek(null);
      lastInteract.current = Date.now();
    }).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, [setPeek]);

  // Keep the native hit-test region in sync with the animated island.
  useEffect(() => {
    const element = rootRef.current;
    if (!element) return;

    let raf = 0;
    const send = () => {
      raf = 0;
      if (concealedRef.current) {
        void invoke("set_hit_rect", {
          rect: { x: 0, y: 0, width: 0, height: 0, radius: 0 },
        }).catch(() => {});
        return;
      }
      const rect = element.getBoundingClientRect();
      const style = window.getComputedStyle(element);
      const radius = Number.parseFloat(style.borderTopLeftRadius) || 0;
      void invoke("set_hit_rect", {
        rect: {
          x: rect.x,
          y: rect.y,
          width: rect.width,
          height: rect.height,
          radius,
        },
      }).catch(() => {});
    };
    const schedule = () => {
      if (!raf) raf = window.requestAnimationFrame(send);
    };

    const observer = new ResizeObserver(schedule);
    observer.observe(element);
    window.addEventListener("resize", schedule);
    schedule();

    return () => {
      observer.disconnect();
      window.removeEventListener("resize", schedule);
      if (raf) window.cancelAnimationFrame(raf);
    };
  }, []);

  useEffect(() => {
    const element = rootRef.current;
    if (!element) return;
    const id = window.requestAnimationFrame(() => {
      if (concealedRef.current) {
        void invoke("set_hit_rect", {
          rect: { x: 0, y: 0, width: 0, height: 0, radius: 0 },
        }).catch(() => {});
        return;
      }
      const rect = element.getBoundingClientRect();
      const style = window.getComputedStyle(element);
      const radius = Number.parseFloat(style.borderTopLeftRadius) || 0;
      void invoke("set_hit_rect", {
        rect: {
          x: rect.x,
          y: rect.y,
          width: rect.width,
          height: rect.height,
          radius,
        },
      }).catch(() => {});
    });
    return () => window.cancelAnimationFrame(id);
  }, [expanded, tab, kind, concealed]);

  // The window is much larger than the island, so an expanded panel near a
  // screen edge would be clipped. Nudge the window until the panel fits; the
  // repeated pass also covers dragging the panel by its grip.
  useEffect(() => {
    if (!isTauri || !expanded) return;
    const dims = EXPANDED[tab];
    const ensure = () => {
      void invoke("ensure_island_visible", {
        width: dims.w,
        height: dims.h,
      }).catch(() => {});
    };
    const id = window.setTimeout(ensure, 60);
    const interval = window.setInterval(ensure, 900);
    window.addEventListener("pointerup", ensure);
    return () => {
      window.clearTimeout(id);
      window.clearInterval(interval);
      window.removeEventListener("pointerup", ensure);
    };
  }, [expanded, tab]);

  const onEnter = () => {
    if (concealed) return;
    interactive.current = true;
    lastInteract.current = Date.now();
    if (!expandedRef.current && settings.expandOnHover) {
      if (hoverTimer.current) window.clearTimeout(hoverTimer.current);
      hoverTimer.current = window.setTimeout(() => {
        if (interactive.current && !expandedRef.current) {
          open(hoverTab.current ?? priorityTab);
        }
      }, 240);
    }
  };

  const onLeave = () => {
    interactive.current = false;
    hoverTab.current = null;
    if (hoverTimer.current) {
      window.clearTimeout(hoverTimer.current);
      hoverTimer.current = null;
    }
    if (!expandedRef.current) return;
    if (pinnedRef.current) return;
    if (collapseTimer.current) window.clearTimeout(collapseTimer.current);
    collapseTimer.current = window.setTimeout(() => {
      if (interactive.current) return;
      const el = rootRef.current;
      if (el && el.contains(document.activeElement)) return;
      if (!pinnedRef.current) collapse();
    }, 650);
  };

  const onClick = () => {
    if (suppressClick.current) {
      suppressClick.current = false;
      dragRef.current = null;
      return;
    }
    if (concealed) return;
    if (!expanded) {
      setPinned(true);
      open(peek ? peek.tab : priorityTab);
    }
  };

  const subtitle = (() => {
    switch (tab) {
      case "media":
        return media.active
          ? media.album || sourceLabel(media.source)
          : "Nothing playing";
      case "timer": {
        if (timer.running) {
          return `${fmtDuration(timerRemaining(timer, now))} left`;
        }
        if (timerRemaining(timer, now) <= 0) return "Complete";
        if (timer.remainingMs < timer.durationMs) return "Paused";
        return `${Math.max(1, Math.round(timer.durationMs / 60_000))} min session`;
      }
      case "tasks":
        return `${openTasks} open · ${doneTasks} done`;
      case "notes": {
        const count = notes.length;
        return count === 0
          ? "No notes yet"
          : `${count} note${count === 1 ? "" : "s"} · newest first`;
      }
      case "calendar": {
        const live = currentEvent(calendar.events, now);
        const next = live ?? upcomingEvent(calendar.events, now);
        if (!calendar.source) return "Connect an iCal feed";
        if (!next) return "No upcoming events";
        if (live) return `Now · ${live.title}`;
        return `${next.allDay ? "All day" : fmtClock(next.start)} · ${next.title}`;
      }
      case "agents":
        return `${activeAgents.length} active · ${agents.length} total`;
      default:
        return `v${info?.version ?? "0.1.0"}`;
    }
  })();

  const meta = TAB_META[tab];
  const HeaderIcon = meta.icon;

  const artwork = artworkMap[media.trackKey];
  const peekMeta = peek ? TAB_META[peek.tab] : null;
  const PeekIcon = peekMeta?.icon ?? Music2;

  return (
    <div
      ref={rootRef}
      className={`island ${expanded ? "is-expanded" : ""} ${
        settings.translucent ? "is-glass" : ""
      } ${concealed ? "is-concealed" : ""} ${dragging ? "is-dragging" : ""}`}
      style={{
        width: dims.w,
        height: dims.h,
        borderRadius: dims.r,
      }}
      onMouseEnter={onEnter}
      onMouseLeave={onLeave}
      onPointerDown={onRootPointerDown}
      onPointerMove={onRootPointerMove}
      onPointerUp={onRootPointerUp}
      onPointerCancel={onRootPointerUp}
      onWheel={() => {
        lastInteract.current = Date.now();
      }}
      onClick={onClick}
      aria-hidden={concealed}
    >
      <div className="island-layer collapsed-layer">
        {kind === "idle" && (
          <div className="collapsed idle">
            {settings.showClock ? (
              <>
                <span className="idle-clock">{fmtClock(now)}</span>
                <span className="idle-dot" />
              </>
            ) : (
              <span className="idle-dot" />
            )}
          </div>
        )}

        {kind === "media" && (
          <div className="collapsed media">
            {artwork ? (
              <img className="art-xs" src={artwork} alt="" draggable={false} />
            ) : (
              <div className="art-xs art-fallback">
                <Music2 size={11} />
              </div>
            )}
            <div className="cl-text">
              <span className="cl-strong">{media.title || "Media"}</span>
              {media.artist && <span className="cl-muted"> · {media.artist}</span>}
            </div>
            <Eq playing={media.playing} />
          </div>
        )}

        {kind === "timer" && (
          <div className="collapsed timer">
            <MiniRing
              progress={timerProgress(timer, now)}
              color={timerRemaining(timer, now) <= 0 ? "#30d158" : "#ff9f0a"}
              size={17}
            />
            <span className="cl-strong timer-cl">
              {fmtDuration(timerRemaining(timer, now))}
            </span>
            <span className="cl-muted">
              {timer.running ? "Focus" : timerRemaining(timer, now) <= 0 ? "Done" : "Paused"}
            </span>
          </div>
        )}

        {kind === "calendar" && calendarNext && (
          <div className="collapsed calendar">
            <span className="cal-pill-icon">
              <CalendarDays size={13} />
            </span>
            <span className="cl-strong">
              {calendarNext.allDay
                ? "All day"
                : calendarNext.start <= now
                  ? "Now"
                  : fmtClock(calendarNext.start)}
            </span>
            <span className="cl-muted cal-pill-title">{calendarNext.title}</span>
          </div>
        )}

        {kind === "agents" && (
          <div className="collapsed agents">
            <div className="avatar-stack">
              {activeAgents.slice(0, 2).map((agent) => (
                <Monogram key={agent.id} agent={agent.agent} size={18} />
              ))}
            </div>
            <div className="cl-text">
              <span className="cl-strong">
                {activeAgents.length} agent{activeAgents.length === 1 ? "" : "s"}
              </span>
              <span className="cl-muted">
                {" "}
                ·{" "}
                {waitingAgents.length > 0
                  ? "waiting"
                  : workingAgents.length > 0
                    ? "working"
                    : "thinking"}
              </span>
            </div>
            <StatusDot
              status={
                waitingAgents.length > 0
                  ? "waiting"
                  : workingAgents.length > 0
                    ? "working"
                    : "thinking"
              }
              size={6}
            />
          </div>
        )}

        {kind === "multi" && (
          <div className="collapsed cluster">
            {liveMedia && (
              <button
                type="button"
                className="cluster-seg seg-media"
                title={
                  media.artist
                    ? `${media.title} — ${media.artist}`
                    : media.title || "Media"
                }
                onPointerEnter={() => {
                  hoverTab.current = "media";
                }}
                onPointerLeave={() => {
                  hoverTab.current = null;
                }}
                onClick={(event) => {
                  event.stopPropagation();
                  open("media");
                }}
              >
                {artwork ? (
                  <img
                    className="seg-art"
                    src={artwork}
                    alt=""
                    draggable={false}
                  />
                ) : (
                  <span className="seg-art art-fallback">
                    <Music2 size={9} />
                  </span>
                )}
                <span className="seg-title">{media.title || "Media"}</span>
                <Eq playing={media.playing} />
              </button>
            )}

            {liveMedia && (liveTimer || liveCalendar || liveAgents) && (
              <i className="cluster-divider" />
            )}

            {liveTimer && (
              <button
                type="button"
                className="cluster-seg seg-timer"
                title="Focus timer"
                onPointerEnter={() => {
                  hoverTab.current = "timer";
                }}
                onPointerLeave={() => {
                  hoverTab.current = null;
                }}
                onClick={(event) => {
                  event.stopPropagation();
                  open("timer");
                }}
              >
                <MiniRing
                  progress={timerProgress(timer, now)}
                  color={
                    timerRemaining(timer, now) <= 0 ? "#30d158" : "#ff9f0a"
                  }
                  size={16}
                />
                <span className="seg-time">
                  {fmtDuration(timerRemaining(timer, now))}
                </span>
              </button>
            )}

            {liveTimer && (liveCalendar || liveAgents) && (
              <i className="cluster-divider" />
            )}

            {liveCalendar && calendarNext && (
              <button
                type="button"
                className="cluster-seg seg-calendar"
                title={calendarNext.title}
                onPointerEnter={() => {
                  hoverTab.current = "calendar";
                }}
                onPointerLeave={() => {
                  hoverTab.current = null;
                }}
                onClick={(event) => {
                  event.stopPropagation();
                  open("calendar");
                }}
              >
                <CalendarDays size={12} className="seg-cal-icon" />
                <span className="seg-time">
                  {calendarNext.allDay
                    ? "All day"
                    : calendarNext.start <= now
                      ? "Now"
                      : fmtClock(calendarNext.start)}
                </span>
              </button>
            )}

            {liveCalendar && liveAgents && <i className="cluster-divider" />}

            {liveAgents && (
              <button
                type="button"
                className="cluster-seg seg-agents"
                title={`${activeAgents.length} active agent${
                  activeAgents.length === 1 ? "" : "s"
                }`}
                onPointerEnter={() => {
                  hoverTab.current = "agents";
                }}
                onPointerLeave={() => {
                  hoverTab.current = null;
                }}
                onClick={(event) => {
                  event.stopPropagation();
                  open("agents");
                }}
              >
                <div className="avatar-stack">
                  {activeAgents.slice(0, 2).map((agent) => (
                    <Monogram key={agent.id} agent={agent.agent} size={15} />
                  ))}
                </div>
                <span className="seg-count">{activeAgents.length}</span>
                <StatusDot
                  status={
                    waitingAgents.length > 0
                      ? "waiting"
                      : workingAgents.length > 0
                        ? "working"
                        : "thinking"
                  }
                  size={5}
                />
              </button>
            )}
          </div>
        )}

        {kind === "peek" && peek && (
          <div className="collapsed peek">
            <div
              className="peek-icon"
              style={{
                background: `${peekMeta?.color ?? "#fff"}1f`,
                color: peekMeta?.color ?? "#fff",
              }}
            >
              <PeekIcon size={15} />
            </div>
            <div className="peek-text">
              <div className="peek-title">{peek.title}</div>
              <div className="peek-sub">{peek.subtitle}</div>
            </div>
            <span className="peek-hint">Open</span>
          </div>
        )}
      </div>

      <div className="island-layer expanded-layer">
        <div className="panel">
          <header className="panel-head">
            <div
              className="panel-icon"
              style={{
                background: `${meta.color}1f`,
                color: meta.color,
              }}
            >
              <HeaderIcon size={15} />
            </div>
            <div className="panel-titles">
              <div className="panel-title">{meta.title}</div>
              <div className="panel-sub">{subtitle}</div>
            </div>
            <div className="panel-actions">
              <button
                className="icon-btn grip"
                title="Drag island"
                onPointerDown={(event) => {
                  event.preventDefault();
                  event.stopPropagation();
                  startNativeDrag();
                }}
                onClick={(event) => event.stopPropagation()}
              >
                <GripVertical size={14} />
              </button>
              <button
                className="icon-btn"
                title="Collapse (Esc)"
                onClick={(event) => {
                  event.stopPropagation();
                  collapse();
                }}
              >
                <ChevronUp size={15} />
              </button>
            </div>
          </header>

          <div className="panel-body tab-content" key={tab}>
            {tab === "media" && <MediaPanel />}
            {tab === "timer" && <TimerPanel />}
            {tab === "tasks" && <TasksPanel />}
            {tab === "notes" && <NotesPanel />}
            {tab === "calendar" && (
              <CalendarPanel
                onOpenSettings={() => {
                  setTab("settings");
                  setPinned(true);
                  lastInteract.current = Date.now();
                }}
              />
            )}
            {tab === "agents" && (
              <AgentsPanel
                onOpenSettings={() => {
                  setTab("settings");
                  setPinned(true);
                  lastInteract.current = Date.now();
                }}
              />
            )}
            {tab === "settings" && <SettingsPanel />}
          </div>

          <nav className="tabs">
            {TAB_ORDER.map((item) => {
              const TabIcon = TAB_META[item].icon;
              return (
                <button
                  key={item}
                  className={`tab ${tab === item ? "active" : ""}`}
                  title={TAB_META[item].label}
                  onClick={(event) => {
                    event.stopPropagation();
                    setTab(item);
                    setPinned(true);
                    lastInteract.current = Date.now();
                  }}
                >
                  <TabIcon size={15} />
                  {showLiveDots && liveTabs[item] && (
                    <span
                      className="tab-live"
                      style={{
                        background: TAB_META[item].color,
                        color: TAB_META[item].color,
                      }}
                    />
                  )}
                </button>
              );
            })}
          </nav>

          {toast && <div className="toast">{toast}</div>}
        </div>
      </div>
    </div>
  );
}
