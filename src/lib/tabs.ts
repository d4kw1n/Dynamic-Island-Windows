import type { ComponentType } from "react";
import {
  Bot,
  CalendarDays,
  ListChecks,
  Music2,
  Settings2,
  StickyNote,
  Timer,
} from "lucide-react";
import type { Tab } from "../types";

export type IconComponent = ComponentType<{
  size?: number | string;
  strokeWidth?: number | string;
  className?: string;
  fill?: string;
}>;

export interface TabMeta {
  label: string;
  title: string;
  icon: IconComponent;
  color: string;
}

export const TAB_META: Record<Tab, TabMeta> = {
  media: { label: "Media", title: "Now playing", icon: Music2, color: "#ff375f" },
  timer: { label: "Timer", title: "Focus timer", icon: Timer, color: "#ff9f0a" },
  tasks: { label: "Tasks", title: "Today", icon: ListChecks, color: "#0a84ff" },
  notes: { label: "Notes", title: "Notes", icon: StickyNote, color: "#ffd60a" },
  calendar: {
    label: "Calendar",
    title: "Schedule",
    icon: CalendarDays,
    color: "#5e5ce6",
  },
  agents: { label: "Agents", title: "Agents", icon: Bot, color: "#30d158" },
  settings: {
    label: "Settings",
    title: "Settings",
    icon: Settings2,
    color: "#98989f",
  },
};

export const TAB_ORDER: Tab[] = [
  "media",
  "timer",
  "tasks",
  "notes",
  "calendar",
  "agents",
  "settings",
];
