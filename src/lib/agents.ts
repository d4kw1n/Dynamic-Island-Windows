export interface AgentBrand {
  name: string;
  letter: string;
  color: string;
  bg: string;
}

export const AGENT_BRAND: Record<string, AgentBrand> = {
  zcode: { name: "ZCode", letter: "Z", color: "#d9ccff", bg: "#7c4dff" },
  claude: { name: "Claude", letter: "C", color: "#ffe0d2", bg: "#c96442" },
  codex: { name: "Codex", letter: "X", color: "#e8eaee", bg: "#4b5563" },
  antigravity: { name: "Antigravity", letter: "A", color: "#d7e6ff", bg: "#2f6fed" },
  chatgpt: { name: "ChatGPT", letter: "G", color: "#d3fbef", bg: "#0f9d78" },
  gemini: { name: "Gemini", letter: "G", color: "#e0e6ff", bg: "#5b6cf0" },
  generic: { name: "Agent", letter: "A", color: "#e5e7eb", bg: "#5b616b" },
};

export function brandFor(agent: string): AgentBrand {
  return AGENT_BRAND[agent] ?? AGENT_BRAND.generic;
}

export function agentStatusColor(status: string): string {
  switch (status) {
    case "working":
      return "var(--green)";
    case "thinking":
      return "var(--blue)";
    case "waiting":
      return "var(--orange)";
    case "error":
      return "var(--red)";
    case "done":
      return "var(--green)";
    default:
      return "var(--faint)";
  }
}

export function agentStatusText(status: string): string {
  switch (status) {
    case "working":
      return "Working";
    case "thinking":
      return "Thinking";
    case "waiting":
      return "Waiting";
    case "error":
      return "Error";
    case "done":
      return "Done";
    default:
      return "Idle";
  }
}
