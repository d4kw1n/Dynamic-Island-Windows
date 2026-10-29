import { useEffect, useState } from "react";
import { Bot, Plug } from "lucide-react";
import { useNow } from "../hooks/useNow";
import { invoke } from "../lib/bridge";
import { brandFor, agentStatusText } from "../lib/agents";
import { fmtElapsed } from "../lib/format";
import { useStore } from "../store/useStore";
import type { AdapterStatus, AgentSession } from "../types";
import { Monogram, StatusDot } from "./Bits";

const HOOK_IDS = new Set(["claude", "zcode", "codex"]);

function detailLine(session: AgentSession): string {
  if (session.status === "waiting") {
    return session.lastMessage || "Waiting for input";
  }
  if (session.status === "done" || session.status === "error") {
    return session.lastMessage || session.task || "Finished";
  }
  if (session.status === "thinking") {
    return session.task || "Thinking…";
  }
  if (session.tool) {
    return session.detail ? `${session.tool} · ${session.detail}` : session.tool;
  }
  return session.task || session.detail || "Working…";
}

function AgentCard({ session, now }: { session: AgentSession; now: number }) {
  const brand = brandFor(session.agent);
  const elapsed = Math.max(
    0,
    (session.completedAt ?? now) - session.startedAt,
  );
  const active = session.status === "working" || session.status === "waiting";
  const determinate =
    session.progress !== null && session.progress !== undefined;
  const pending = session.status === "waiting" && Boolean(session.requestId);

  const resolve = (
    decision: "allow" | "allow_always" | "allow_tool" | "deny",
  ) => {
    if (!session.requestId) return;
    void invoke("resolve_permission", {
      requestId: session.requestId,
      decision,
    }).catch(() => {});
  };

  return (
    <div
      className={`agent-card ${session.status}`}
      title={[session.cwd, session.model].filter(Boolean).join("\n")}
      data-request={session.requestId || undefined}
    >
      <Monogram agent={session.agent} size={30} />
      <div className="agent-main">
        <div className="agent-title">{session.title || brand.name}</div>
        <div className="agent-line">{detailLine(session)}</div>
      </div>
      <div className="agent-side">
        <span className={`status-chip ${session.status}`}>
          <StatusDot status={session.status} />
          {agentStatusText(session.status)}
        </span>
        <span className="agent-elapsed">{fmtElapsed(elapsed)}</span>
      </div>

      {pending && (
        <div className="perm-box">
          <div className="perm-title">
            {brand.name} wants to use {session.tool || "a tool"}
          </div>
          {(session.detail || session.lastMessage) && (
            <code className="perm-cmd">
              {session.detail || session.lastMessage}
            </code>
          )}
          <div className="perm-actions">
            <button className="btn-small" onClick={() => resolve("deny")}>
              Deny
            </button>
            <button
              className="btn-small"
              title="Allow this exact command or path in this project"
              onClick={() => resolve("allow_always")}
            >
              Always allow
            </button>
            <button
              className="btn-small"
              title={`Allow every ${session.tool || "tool"} use in this project`}
              onClick={() => resolve("allow_tool")}
            >
              Allow all {session.tool || "tool"}
            </button>
            <button
              className="btn-small primary"
              onClick={() => resolve("allow")}
            >
              Allow once
            </button>
          </div>
        </div>
      )}

      {determinate && session.status !== "done" ? (
        <div className="agent-progress determinate">
          <i
            className={`bar ${session.status}`}
            style={{
              width: `${Math.round(
                Math.min(1, Math.max(0, session.progress ?? 0)) * 100,
              )}%`,
            }}
          />
        </div>
      ) : active ? (
        <div className="agent-progress indeterminate">
          <i />
        </div>
      ) : null}
    </div>
  );
}

export function AgentsPanel({
  onOpenSettings,
}: {
  onOpenSettings: () => void;
}) {
  const agents = useStore((s) => s.agents);
  const sources = useStore((s) => s.sources);
  const info = useStore((s) => s.info);
  const now = useNow(1000);

  const [adapters, setAdapters] = useState<AdapterStatus[] | null>(null);

  useEffect(() => {
    let alive = true;
    void invoke<AdapterStatus[]>("adapter_status")
      .then((value) => {
        if (alive) setAdapters(value);
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, []);

  const active = agents.filter(
    (agent) =>
      agent.status === "working" ||
      agent.status === "waiting" ||
      agent.status === "thinking",
  );
  const recent = agents.filter((agent) => !active.includes(agent));

  // With many sessions the permission card can be below the fold; bring it
  // into view as soon as a request arrives.
  const pendingId = agents.find((agent) => agent.requestId)?.requestId ?? "";
  useEffect(() => {
    if (!pendingId) return;
    const card = document.querySelector<HTMLElement>(
      `[data-request="${pendingId}"]`,
    );
    card?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [pendingId]);
  const detected = sources.filter(
    (source) => !agents.some((agent) => agent.agent === source.id),
  );
  const connected = (adapters ?? []).filter((adapter) => adapter.installed);
  const stale = connected.filter((adapter) => !adapter.current);
  const connectedNames = connected.map(
    (adapter) => brandFor(adapter.id).name,
  );
  const staleNames = stale.map((adapter) => brandFor(adapter.id).name);
  const empty = agents.length === 0 && detected.length === 0;
  const footNote =
    stale.length > 0
      ? `Reconnect hooks: ${staleNames.join(", ")}`
      : connected.length > 0
        ? `Hooks: ${connectedNames.join(", ")}`
        : detected.length > 0
          ? `Monitoring: ${detected
              .map((source) => brandFor(source.id).name)
              .join(", ")}`
          : `Webhook :${info?.port ?? 17321}`;

  const demo = () => {
    void invoke("run_demo").catch(() => {});
  };

  const clear = () => {
    void invoke("clear_agents").catch(() => {});
  };

  return (
    <div className="agents-panel">
      {empty ? (
        <div className="empty">
          <div className="empty-icon">
            <Bot size={20} />
          </div>
          <div className="empty-title">
            {connected.length > 0 ? "Hooks ready" : "No active agents"}
          </div>
          <div className="empty-sub">
            {connected.length > 0
              ? `${connectedNames.join(
                  " and ",
                )} connected. Start a new session to stream live activity here.`
              : "Connect Claude Code, ZCode or Codex from Settings. Any other tool can post to the local webhook."}
          </div>
          <div className="empty-actions">
            <button className="btn-small primary" onClick={onOpenSettings}>
              <Plug size={11} /> Connect
            </button>
            <button className="btn-small" onClick={demo}>
              Run demo
            </button>
          </div>
        </div>
      ) : (
        <div className="agent-list">
          {detected.length > 0 && (
            <div className="section-label">Detected · not reporting yet</div>
          )}
          {detected.map((source) => (
            <div className="agent-card ghost" key={`ghost:${source.id}`}>
              <Monogram agent={source.id} size={30} />
              <div className="agent-main">
                <div className="agent-title">{brandFor(source.id).name}</div>
                <div className="agent-line">
                  {HOOK_IDS.has(source.id)
                    ? "Running · waiting for hook events"
                    : "Running · no hook API, monitoring only"}
                </div>
              </div>
              <span className="status-chip idle">Detected</span>
            </div>
          ))}
          {active.map((session) => (
            <AgentCard key={session.id} session={session} now={now} />
          ))}
          {recent.length > 0 && (
            <div className="section-label">Recent · {recent.length}</div>
          )}
          {recent.map((session) => (
            <AgentCard key={session.id} session={session} now={now} />
          ))}
          {agents.length === 0 && connected.length > 0 && (
            <div className="agent-note">
              {stale.length > 0
                ? `${staleNames.join(
                    " / ",
                  )} hooks point at another build. Open Settings and press Reconnect.`
                : `Restart ${connectedNames.join(
                    " / ",
                  )} to load the hooks. The running session started before they were added.`}
            </div>
          )}
        </div>
      )}

      <div className="agents-foot">
        <span className="foot-note">{footNote}</span>
        <div className="foot-actions">
          {agents.length > 0 && (
            <button className="link-btn" onClick={clear}>
              Clear
            </button>
          )}
          <button className="link-btn" onClick={demo}>
            Demo
          </button>
        </div>
      </div>
    </div>
  );
}
