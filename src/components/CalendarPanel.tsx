import { useState } from "react";
import {
  CalendarDays,
  CalendarPlus,
  MapPin,
  RefreshCw,
  TriangleAlert,
} from "lucide-react";
import { useNow } from "../hooks/useNow";
import { invoke } from "../lib/bridge";
import {
  countdownLabel,
  durationLabel,
  eventGroup,
} from "../lib/calendar";
import { fmtAgo, fmtClock } from "../lib/format";
import { useStore } from "../store/useStore";
import type { CalendarEvent } from "../types";

function EventRow({ event, now }: { event: CalendarEvent; now: number }) {
  const live = event.start <= now && event.end > now;
  const countdown = countdownLabel(event, now);
  const duration = durationLabel(event);
  const showMeta = Boolean(event.location || event.calendar);

  return (
    <div className={`cal-row ${live ? "live" : ""}`}>
      <div className="cal-time">
        <span className="cal-time-main">
          {event.allDay ? "All day" : fmtClock(event.start)}
        </span>
        {duration && <span className="cal-time-sub">{duration}</span>}
      </div>
      <div className="cal-main">
        <div className="cal-title">{event.title}</div>
        {showMeta && (
          <div className="cal-meta">
            {event.location && (
              <span>
                <MapPin size={9} /> {event.location}
              </span>
            )}
            {event.calendar && <span className="cal-name">{event.calendar}</span>}
          </div>
        )}
      </div>
      {countdown && (
        <span className={`cal-badge ${live ? "live" : ""}`}>{countdown}</span>
      )}
    </div>
  );
}

export function CalendarPanel({
  onOpenSettings,
}: {
  onOpenSettings: () => void;
}) {
  const calendar = useStore((s) => s.calendar);
  const settings = useStore((s) => s.settings);
  const showToast = useStore((s) => s.showToast);
  const now = useNow(15_000);
  const [busy, setBusy] = useState(false);

  // Google Calendar has no iCal URL; its snapshot carries the source instead.
  const connected = Boolean(settings.calendarUrl) || Boolean(calendar.source);
  const events = calendar.events;
  const error = calendar.error;
  const syncing = calendar.updatedAt === null && connected && !error;

  const sync = async () => {
    if (busy) return;
    setBusy(true);
    showToast("Refreshing calendar…");
    try {
      await invoke("calendar_refresh");
    } catch (reason) {
      showToast(String(reason));
    } finally {
      window.setTimeout(() => setBusy(false), 900);
    }
  };

  if (!connected) {
    return (
      <div className="calendar-panel">
        <div className="empty">
          <div className="empty-icon cal">
            <CalendarDays size={20} />
          </div>
          <div className="empty-title">Calendar not connected</div>
          <div className="empty-sub">
            Subscribe to a read-only iCal link from Google Calendar, Outlook or
            iCloud. Events appear here and the island reminds you before they
            start.
          </div>
          <div className="empty-actions">
            <button className="btn-small primary" onClick={onOpenSettings}>
              <CalendarPlus size={11} /> Connect calendar
            </button>
          </div>
        </div>
        <div className="cal-foot">
          <span className="foot-note">Works with any .ics feed or local file</span>
        </div>
      </div>
    );
  }

  if (events.length === 0) {
    return (
      <div className="calendar-panel">
        <div className="empty">
          <div className="empty-icon cal">
            {error ? <TriangleAlert size={20} /> : <CalendarDays size={20} />}
          </div>
          <div className="empty-title">
            {error ? "Could not load the calendar" : syncing ? "Loading schedule" : "No upcoming events"}
          </div>
          <div className="empty-sub">
            {error ??
              (syncing
                ? "Fetching your feed for the next 14 days."
                : "The next two weeks are clear.")}
          </div>
          <div className="empty-actions">
            <button className="btn-small" onClick={sync} disabled={busy}>
              <RefreshCw size={11} /> Refresh
            </button>
            <button className="btn-small" onClick={onOpenSettings}>
              Settings
            </button>
          </div>
        </div>
        <div className="cal-foot">
          <span className="foot-note">
            {calendar.updatedAt
              ? `Synced ${fmtAgo(calendar.updatedAt, Date.now())}`
              : "Waiting for the first sync"}
          </span>
        </div>
      </div>
    );
  }

  const groups: Array<{ label: string; items: CalendarEvent[] }> = [];
  for (const event of events) {
    const label = eventGroup(event, now);
    const last = groups[groups.length - 1];
    if (last && last.label === label) last.items.push(event);
    else groups.push({ label, items: [event] });
  }

  return (
    <div className="calendar-panel">
      {error && (
        <div className="cal-warning">
          <TriangleAlert size={11} />
          <span>{error} · showing the last known schedule</span>
        </div>
      )}

      <div className="cal-list">
        {groups.map((group) => (
          <div className="cal-group" key={group.label}>
            <div className="section-label">{group.label}</div>
            {group.items.map((event) => (
              <EventRow key={event.id} event={event} now={now} />
            ))}
          </div>
        ))}
      </div>

      <div className="cal-foot">
        <span className="foot-note">
          {events.length} event{events.length === 1 ? "" : "s"}
          {settings.calendarRemind > 0
            ? ` · reminders ${settings.calendarRemind}m before`
            : " · reminders off"}
        </span>
        <div className="foot-actions">
          <button className="link-btn" onClick={sync} disabled={busy}>
            <RefreshCw size={10} /> Sync
          </button>
        </div>
      </div>
    </div>
  );
}
