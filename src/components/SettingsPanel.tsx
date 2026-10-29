import { useEffect, useState } from "react";
import {
  CalendarDays,
  Check,
  Copy,
  FolderOpen,
  LocateFixed,
  Power,
  RefreshCw,
  Terminal,
  Trash2,
} from "lucide-react";
import { invoke, listen } from "../lib/bridge";
import { fmtAgo } from "../lib/format";
import { useStore } from "../store/useStore";
import type { AdapterStatus, GoogleStatus } from "../types";
import { Monogram, Toggle } from "./Bits";

const ADAPTERS = [
  {
    id: "claude",
    name: "Claude Code",
    desc: "Session, tool & permission hooks",
  },
  {
    id: "zcode",
    name: "ZCode",
    desc: "Seven lifecycle hooks",
  },
  {
    id: "codex",
    name: "Codex",
    desc: "Turn completion via notify",
  },
];

const REMIND_CHOICES = [0, 5, 10, 15, 30];

export function SettingsPanel() {
  const settings = useStore((s) => s.settings);
  const calendar = useStore((s) => s.calendar);
  const updateSettings = useStore((s) => s.updateSettings);
  const setWindowPos = useStore((s) => s.setWindowPos);
  const info = useStore((s) => s.info);
  const showToast = useStore((s) => s.showToast);

  const [adapters, setAdapters] = useState<AdapterStatus[]>([]);
  const [autostart, setAutostart] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [calendarDraft, setCalendarDraft] = useState(settings.calendarUrl);
  const [calendarBusy, setCalendarBusy] = useState(false);
  const [google, setGoogle] = useState<GoogleStatus>({
    connected: false,
    error: null,
  });
  const [googleId, setGoogleId] = useState("");
  const [googleSecret, setGoogleSecret] = useState("");
  const [googleBusy, setGoogleBusy] = useState(false);

  useEffect(() => {
    setCalendarDraft(settings.calendarUrl);
  }, [settings.calendarUrl]);

  const url = `http://127.0.0.1:${info?.port ?? 17321}/v1/agent/event`;

  useEffect(() => {
    let alive = true;
    void invoke<AdapterStatus[]>("adapter_status")
      .then((value) => {
        if (alive) setAdapters(value);
      })
      .catch(() => {});
    void invoke<boolean>("get_autostart")
      .then((value) => {
        if (!alive) return;
        setAutostart(value);
        updateSettings({ launchAtLogin: value });
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [updateSettings]);

  useEffect(() => {
    let alive = true;
    void invoke<GoogleStatus>("google_status")
      .then((value) => {
        if (alive && value) setGoogle(value);
      })
      .catch(() => {});
    let unlisten: (() => void) | undefined;
    void listen<GoogleStatus>("google://status", (value) => {
      if (value) setGoogle(value);
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);

  const connectGoogle = async () => {
    if (googleBusy) return;
    setGoogleBusy(true);
    try {
      await invoke("google_connect", {
        clientId: googleId.trim(),
        clientSecret: googleSecret.trim(),
      });
      showToast("Authorize Dynamic Island in your browser");
    } catch (error) {
      showToast(String(error));
    } finally {
      window.setTimeout(() => setGoogleBusy(false), 900);
    }
  };

  const disconnectGoogle = async () => {
    if (googleBusy) return;
    setGoogleBusy(true);
    try {
      await invoke("google_disconnect");
      setGoogle({ connected: false, error: null });
      showToast("Google Calendar disconnected");
    } catch (error) {
      showToast(String(error));
    } finally {
      setGoogleBusy(false);
    }
  };

  const install = async (id: string) => {
    setBusy(id);
    try {
      const message = await invoke<string>("install_adapter", { id });
      showToast(message);
      const fresh = await invoke<AdapterStatus[]>("adapter_status");
      setAdapters(fresh);
    } catch (error) {
      showToast(String(error));
    } finally {
      setBusy(null);
    }
  };

  const disconnect = async (id: string) => {
    setBusy(id);
    try {
      const message = await invoke<string>("uninstall_adapter", { id });
      showToast(message);
      const fresh = await invoke<AdapterStatus[]>("adapter_status");
      setAdapters(fresh);
    } catch (error) {
      showToast(String(error));
    } finally {
      setBusy(null);
    }
  };

  const toggleAutostart = async (next: boolean) => {
    setAutostart(next);
    updateSettings({ launchAtLogin: next });
    try {
      await invoke("set_autostart", { enabled: next });
    } catch (error) {
      setAutostart(!next);
      updateSettings({ launchAtLogin: !next });
      showToast(String(error));
    }
  };

  const copyUrl = async () => {
    try {
      await navigator.clipboard.writeText(url);
      showToast("Webhook URL copied");
    } catch {
      showToast(url);
    }
  };

  const openData = () => {
    void invoke("open_data_dir").catch(() => {});
  };

  const quit = () => {
    void invoke("quit_app").catch(() => {});
  };

  const center = async () => {
    try {
      await invoke("reset_window_position");
      setWindowPos(null);
      showToast("Island centered");
    } catch (error) {
      showToast(String(error));
    }
  };

  const statusFor = (id: string) => adapters.find((item) => item.id === id);

  const saveCalendar = () => {
    const next = calendarDraft.trim();
    updateSettings({ calendarUrl: next });
    showToast(
      next
        ? "Calendar saved · syncing in the background"
        : "Calendar subscription removed",
    );
  };

  const removeCalendar = () => {
    setCalendarDraft("");
    updateSettings({ calendarUrl: "" });
    showToast("Calendar subscription removed");
  };

  const refreshCalendar = async () => {
    if (calendarBusy) return;
    setCalendarBusy(true);
    try {
      await invoke("calendar_refresh");
      showToast("Refreshing calendar…");
    } catch (error) {
      showToast(String(error));
    } finally {
      window.setTimeout(() => setCalendarBusy(false), 1200);
    }
  };

  const calendarConnected =
    Boolean(settings.calendarUrl) || Boolean(calendar.source);
  const calendarStatus = calendar.error
    ? calendar.error
    : calendarConnected
      ? calendar.updatedAt
        ? `${calendar.events.length} upcoming · synced ${fmtAgo(
            calendar.updatedAt,
            Date.now(),
          )}`
        : "Connected · waiting for the first sync"
      : "Read-only iCal link from Google, Outlook or iCloud";

  return (
    <div className="settings-panel">
      <div className="set-group">
        <div className="set-group-title">General</div>
        <div className="set-row">
          <div className="set-main">
            <div className="set-name">Launch at login</div>
            <div className="set-desc">Start the island with Windows</div>
          </div>
          <div className="set-right">
            <Toggle
              on={autostart}
              onChange={toggleAutostart}
              label="Launch at login"
            />
          </div>
        </div>
        <div className="set-row">
          <div className="set-main">
            <div className="set-name">Expand on hover</div>
            <div className="set-desc">Open the panel after a short hover</div>
          </div>
          <div className="set-right">
            <Toggle
              on={settings.expandOnHover}
              onChange={(next) => updateSettings({ expandOnHover: next })}
              label="Expand on hover"
            />
          </div>
        </div>
        <div className="set-row">
          <div className="set-main">
            <div className="set-name">Clock when idle</div>
            <div className="set-desc">Show the time in the collapsed pill</div>
          </div>
          <div className="set-right">
            <Toggle
              on={settings.showClock}
              onChange={(next) => updateSettings({ showClock: next })}
              label="Clock when idle"
            />
          </div>
        </div>
        <div className="set-row">
          <div className="set-main">
            <div className="set-name">Hide over fullscreen</div>
            <div className="set-desc">Tuck away for games and fullscreen video</div>
          </div>
          <div className="set-right">
            <Toggle
              on={settings.hideFullscreen}
              onChange={(next) => updateSettings({ hideFullscreen: next })}
              label="Hide over fullscreen"
            />
          </div>
        </div>
        <div className="set-row">
          <div className="set-main">
            <div className="set-name">Translucent surface</div>
            <div className="set-desc">Let a hint of the desktop show through</div>
          </div>
          <div className="set-right">
            <Toggle
              on={settings.translucent}
              onChange={(next) => updateSettings({ translucent: next })}
              label="Translucent surface"
            />
          </div>
        </div>
        <div className="set-row">
          <div className="set-main">
            <div className="set-name">Island position</div>
            <div className="set-desc">Drag the pill to move it anywhere</div>
          </div>
          <div className="set-right">
            <button className="btn-small" onClick={center}>
              <LocateFixed size={11} /> Center
            </button>
          </div>
        </div>
      </div>

      <div className="set-group">
        <div className="set-group-title">Calendar</div>
        <div className="set-row">
          <div className="set-main">
            <div className="set-name">Google Calendar</div>
            <div className={`set-desc ${google.error ? "warn" : ""}`}>
              {google.connected
                ? "Connected · reading the calendars visible in Google Calendar"
                : google.error ??
                  "Sign in once; used when no iCal link is set"}
            </div>
          </div>
          <div className="set-right">
            {google.connected && (
              <div className="adapter-actions">
                <span className="adapter-on">
                  <Check size={11} /> Connected
                </span>
                <button
                  className="icon-btn tiny"
                  title="Disconnect Google Calendar"
                  onClick={disconnectGoogle}
                  disabled={googleBusy}
                >
                  <Trash2 size={12} />
                </button>
              </div>
            )}
          </div>
        </div>
        {!google.connected && (
          <div className="google-form">
            <input
              className="cal-input"
              placeholder="OAuth client ID"
              value={googleId}
              spellCheck={false}
              onChange={(event) => setGoogleId(event.target.value)}
            />
            <input
              className="cal-input"
              type="password"
              placeholder="Client secret"
              value={googleSecret}
              spellCheck={false}
              onChange={(event) => setGoogleSecret(event.target.value)}
            />
            <button
              className="btn-small primary"
              onClick={connectGoogle}
              disabled={googleBusy || !googleId.trim()}
            >
              {googleBusy ? "Waiting…" : "Connect"}
            </button>
          </div>
        )}
        <div className="adapter-hint">
          Google Cloud Console → Credentials → OAuth client ID → Desktop app.
          Add http://127.0.0.1:17321/oauth/google/callback as a redirect URI.
        </div>
        <div className="cal-input-row">
          <CalendarDays size={13} className="cal-input-icon" />
          <input
            className="cal-input"
            placeholder="…or an iCal link: https://…/basic.ics"
            value={calendarDraft}
            spellCheck={false}
            onChange={(event) => setCalendarDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") saveCalendar();
            }}
          />
          <button
            className="btn-small primary"
            onClick={saveCalendar}
            disabled={calendarDraft.trim() === settings.calendarUrl.trim()}
          >
            Save
          </button>
        </div>
        <div className={`cal-status ${calendar.error ? "bad" : ""}`}>
          <span className="cal-status-text">{calendarStatus}</span>
          {calendarConnected && (
            <span className="cal-status-actions">
              <button
                className="icon-btn tiny"
                title="Sync now"
                onClick={refreshCalendar}
                disabled={calendarBusy}
              >
                <RefreshCw size={11} className={calendarBusy ? "spin" : ""} />
              </button>
              <button
                className="icon-btn tiny"
                title="Remove subscription"
                onClick={removeCalendar}
              >
                <Trash2 size={11} />
              </button>
            </span>
          )}
        </div>
        <div className="set-row">
          <div className="set-main">
            <div className="set-name">Remind me before</div>
            <div className="set-desc">Peek and notification before an event</div>
          </div>
          <div className="set-right reminder-chips">
            {REMIND_CHOICES.map((minutes) => (
              <button
                key={minutes}
                className={`chip ${
                  settings.calendarRemind === minutes ? "on" : ""
                }`}
                onClick={() => updateSettings({ calendarRemind: minutes })}
              >
                {minutes === 0 ? "Off" : `${minutes}m`}
              </button>
            ))}
          </div>
        </div>
        <div className="adapter-hint">
          Prefer an iCal secret address? Google Calendar → Settings → “Secret
          address in iCal format”. Published Outlook/iCloud links and local .ics
          files also work.
        </div>
      </div>

      <div className="set-group">
        <div className="set-group-title">Agent hooks</div>
        {ADAPTERS.map((adapter) => {
          const status = statusFor(adapter.id);
          const installed = status?.installed ?? false;
          const stale = installed && !status?.current;
          const working = busy === adapter.id;
          return (
            <div
              className="adapter-row"
              key={adapter.id}
              title={status?.path}
            >
              <Monogram agent={adapter.id} size={26} />
              <div className="set-main">
                <div className="set-name">{adapter.name}</div>
                <div className={`set-desc ${stale ? "warn" : ""}`}>
                  {stale
                    ? "Hooks point at another build — reconnect"
                    : adapter.desc}
                </div>
              </div>
              {installed ? (
                <div className="adapter-actions">
                  {status?.current ? (
                    <span className="adapter-on">
                      <Check size={11} /> Connected
                    </span>
                  ) : (
                    <button
                      className="btn-small primary"
                      disabled={working}
                      onClick={() => install(adapter.id)}
                    >
                      <RefreshCw size={11} />
                      {working ? "Saving…" : "Reconnect"}
                    </button>
                  )}
                  <button
                    className="icon-btn tiny"
                    title={`Disconnect ${adapter.name}`}
                    disabled={working}
                    onClick={() => disconnect(adapter.id)}
                  >
                    <Trash2 size={12} />
                  </button>
                </div>
              ) : (
                <button
                  className="btn-small primary"
                  disabled={working}
                  onClick={() => install(adapter.id)}
                >
                  {working ? "Connecting…" : "Connect"}
                </button>
              )}
            </div>
          );
        })}

        <div className="adapter-row" title={url}>
          <div className="monogram monogram-generic" style={{ width: 26, height: 26 }}>
            <Terminal size={13} />
          </div>
          <div className="set-main">
            <div className="set-name">Any other agent</div>
            <div className="set-desc">POST events to the local webhook</div>
          </div>
          <button className="btn-small" onClick={copyUrl}>
            <Copy size={11} /> URL
          </button>
        </div>
        <div className="adapter-hint">
          Restart the agent after connecting. ZCode and Claude Code read hooks
          when a session starts.
        </div>
      </div>

      <div className="set-group">
        <div className="set-group-title">Local API</div>
        <div className="api-box">
          <span className="api-url">{url}</span>
          <button className="icon-btn tiny" onClick={copyUrl} title="Copy URL">
            <Copy size={12} />
          </button>
        </div>
        <div className="api-hint">
          JSON: agent, event, session_id, task, tool, detail, message
        </div>
      </div>

      <div className="settings-foot">
        <button className="link-btn" onClick={openData}>
          <FolderOpen size={11} /> Data folder
        </button>
        <button className="link-btn danger" onClick={quit}>
          <Power size={11} /> Quit
        </button>
        <span className="version">
          Dynamic Island {info?.version ?? "0.1.0"}
        </span>
      </div>
    </div>
  );
}
