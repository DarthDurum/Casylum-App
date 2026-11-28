import type { Block, Message } from "./api";

export type ToolUse = Extract<Block, { type: "tool_use" }>;
export type ToolResult = Extract<Block, { type: "tool_result" }>;

export function resultIndex(messages: Message[]) {
  const map = new Map<string, ToolResult>();
  for (const m of messages) for (const b of m.content) if (b.type === "tool_result") map.set(b.tool_use_id, b);
  return map;
}

const short = (s: string, n = 64) => (s.length > n ? s.slice(0, n - 1) + "…" : s);

export function stepLabel(t: ToolUse, r?: ToolResult): { verb: string; target: string; extra?: string } {
  const i = t.input ?? {};
  switch (t.name) {
    case "read_file":
      return { verb: "Read", target: i.path ?? "" };
    case "list_dir":
      return { verb: "Listed", target: i.path && i.path !== "." ? i.path : "project" };
    case "search":
      return { verb: "Searched", target: `“${short(i.pattern ?? "", 40)}”`, extra: i.glob || i.path };
    case "write_file": {
      const m = r?.meta;
      return { verb: m?.created === false ? "Rewrote" : "Created", target: i.path ?? "", extra: m ? `+${m.added ?? 0} −${m.removed ?? 0}` : undefined };
    }
    case "edit_file": {
      const m = r?.meta;
      return { verb: "Edited", target: i.path ?? "", extra: m ? `+${m.added ?? 0} −${m.removed ?? 0}` : undefined };
    }
    case "run_command":
      return { verb: "Ran", target: short(i.command ?? "", 72), extra: r?.meta?.exit_code !== undefined ? `exit ${r.meta.exit_code}` : undefined };
    case "update_plan":
      return { verb: "Updated", target: "plan" };
    case "use_skill":
      return { verb: "Loaded skill", target: i.name ?? "" };
    default: {
      const mcp = mcpParts(t.name, r);
      if (mcp) return { verb: mcp.tool.replace(/_/g, " "), target: firstArg(i), extra: mcp.server };
      return { verb: t.name, target: "" };
    }
  }
}

/** Server and tool of an MCP tool call (`mcp__<server>__<tool>`). */
export function mcpParts(name: string, r?: ToolResult): { server: string; tool: string } | null {
  if (!name.startsWith("mcp__")) return null;
  if (r?.meta?.kind === "mcp") return { server: r.meta.server, tool: r.meta.tool };
  const rest = name.slice(5);
  const i = rest.indexOf("__");
  return i < 0 ? { server: rest, tool: rest } : { server: rest.slice(0, i), tool: rest.slice(i + 2) };
}

/** A short hint of what an MCP call is about: its first string argument. */
function firstArg(input: any): string {
  if (!input || typeof input !== "object") return "";
  for (const v of Object.values(input)) if (typeof v === "string" && v.trim()) return short(v.replace(/\s+/g, " "), 56);
  return "";
}

export interface PlanItem {
  text: string;
  /** Optional one-sentence detail; older chats and some models leave it out. */
  description?: string;
  status: "pending" | "in_progress" | "done";
}

export function latestPlan(messages: Message[]): PlanItem[] {
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i];
    for (let j = m.content.length - 1; j >= 0; j--) {
      const b = m.content[j];
      if (b.type === "tool_use" && b.name === "update_plan" && Array.isArray(b.input?.items)) return b.input.items;
    }
  }
  return [];
}

export interface FileChange {
  path: string;
  before: string | null;
  after: string | null;
  created: boolean;
  added: number;
  removed: number;
  edits: number;
}

/** Collapse every diff in a session into one before/after per file. */
export function sessionChanges(messages: Message[]): FileChange[] {
  const map = new Map<string, FileChange>();
  for (const m of messages)
    for (const b of m.content) {
      if (b.type !== "tool_result" || b.meta?.kind !== "diff") continue;
      const d = b.meta;
      const cur = map.get(d.path);
      if (!cur) {
        map.set(d.path, { path: d.path, before: d.before ?? null, after: d.after ?? null, created: !!d.created, added: d.added ?? 0, removed: d.removed ?? 0, edits: 1 });
      } else {
        cur.after = d.after ?? null;
        cur.added += d.added ?? 0;
        cur.removed += d.removed ?? 0;
        cur.edits += 1;
      }
    }
  return [...map.values()];
}

export function formatTokens(n: number) {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return (n / 1000).toFixed(n < 10_000 ? 1 : 0) + "k";
  return (n / 1_000_000).toFixed(2) + "M";
}
