import type { CalendarEvent } from "../types";

export const CALENDAR_LEAD_MS = 90 * 60_000;

export function currentEvent(
  events: CalendarEvent[],
  now: number,
): CalendarEvent | null {
  return (
    events.find((event) => event.start <= now && event.end > now) ?? null
  );
}

export function upcomingEvent(
  events: CalendarEvent[],
  now: number,
): CalendarEvent | null {
  return events.find((event) => event.start > now) ?? null;
}

/** An event that should own the collapsed pill: in progress or starting soon. */
export function imminentEvent(
  events: CalendarEvent[],
  now: number,
): CalendarEvent | null {
  const live = currentEvent(events, now);
  if (live) return live;
  const next = upcomingEvent(events, now);
  if (next && next.start - now <= CALENDAR_LEAD_MS) return next;
  return null;
}

export function countdownLabel(event: CalendarEvent, now: number): string {
  if (event.allDay) return "";
  if (event.start <= now && event.end > now) return "Now";
  const minutes = Math.round((event.start - now) / 60_000);
  if (minutes <= 0) return "Now";
  if (minutes < 60) return `in ${minutes}m`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `in ${hours}h`;
  return "";
}

export function durationLabel(event: CalendarEvent): string {
  if (event.allDay) return "";
  const minutes = Math.max(1, Math.round((event.end - event.start) / 60_000));
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return rest === 0 ? `${hours}h` : `${hours}h ${rest}m`;
}

export function dayLabel(ts: number, now: number): string {
  const date = new Date(ts);
  const today = new Date(now);
  const tomorrow = new Date(now);
  tomorrow.setDate(today.getDate() + 1);

  const same = (a: Date, b: Date) =>
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate();

  if (same(date, today)) return "Today";
  if (same(date, tomorrow)) return "Tomorrow";
  return date.toLocaleDateString(undefined, {
    weekday: "short",
    month: "short",
    day: "numeric",
  });
}

export function eventGroup(event: CalendarEvent, now: number): string {
  if (event.start <= now && event.end > now) return "Happening now";
  const date = new Date(event.start);
  const today = new Date(now);
  if (
    date.getFullYear() === today.getFullYear() &&
    date.getMonth() === today.getMonth() &&
    date.getDate() === today.getDate()
  ) {
    return "Today";
  }
  return dayLabel(event.start, now);
}
