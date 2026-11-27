// Typed bridge to the Rust core. API keys never come back through here:
// the UI only ever sees whether a key exists and a masked hint.
import { invoke } from "@tauri-apps/api/core";
import type { ModelList } from "./models";

export type Role = "user" | "assistant";

export type Block =
  | { type: "text"; text: string }
  | { type: "thinking"; thinking: string; signature?: string }
  | { type: "redacted_thinking"; data: string }
  | { type: "tool_use"; id: string; name: string; input: any; provider_meta?: any }
  | { type: "tool_result"; tool_use_id: string; content: string; is_error: boolean; meta?: any };

export interface Message {
  role: Role;
  content: Block[];
}

export type PermissionMode = "ask" | "auto_edit" | "full_auto";

export interface Settings {
  provider: string;
  model: string;
  permission_mode: PermissionMode;
  theme: "dark" | "light";
  ollama_base_url: string;
  custom_base_url: string;
  recent_projects: string[];
  disabled_skills: string[];
  auto_update: boolean;
}

export interface Provider {
  id: string;
  name: string;
  needs_key: boolean;
  models: string[];
  key_url: string;
  has_key: boolean;
  key_hint: string | null;
}

export interface Usage {
  input_tokens: number;
  output_tokens: number;
}

export interface SessionSummary {
  id: string;
  project: string;
  title: string;
  updated_at: number;
  model: string;
}

export interface RunInfo {
  id: string;
  files: string[];
  reverted: boolean;
}

export interface Session extends SessionSummary {
  created_at: number;
  provider: string;
  messages: Message[];
  runs: RunInfo[];
  usage: Usage;
}

export interface Skill {
  name: string;
  description: string;
  scope: "global" | "project";
  dir: string;
  enabled: boolean;
}

export interface FsEntry {
  name: string;
  path: string;
  is_dir: boolean;
}

export type Decision = "allow" | "allow_always" | "deny";

export type AgentEvent =
  | { kind: "started"; session_id: string; run_id: string }
  | { kind: "text_delta"; session_id: string; text: string }
  | { kind: "thinking_delta"; session_id: string; text: string }
  | { kind: "tool_started"; session_id: string; name: string }
  | { kind: "message"; session_id: string; message: Message }
  | { kind: "tool_running"; session_id: string; tool_use_id: string }
  | { kind: "tool_result"; session_id: string; block: Extract<Block, { type: "tool_result" }> }
  | { kind: "approval"; session_id: string; request_id: string; tool_use_id: string; name: string; input: any }
  | { kind: "usage"; session_id: string; turn: Usage; total: Usage }
  | { kind: "done"; session_id: string; run_id: string; files_changed: number; stopped: boolean }
  | { kind: "error"; session_id: string; message: string };

export const api = {
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  listProviders: () => invoke<Provider[]>("list_providers"),
  setApiKey: (provider: string, key: string) => invoke<void>("set_api_key", { provider, key }),
  deleteApiKey: (provider: string) => invoke<void>("delete_api_key", { provider }),
  listModels: (provider: string, refresh = false) => invoke<ModelList>("list_models", { provider, refresh }),

  addRecentProject: (path: string) => invoke<Settings>("add_recent_project", { path }),
  removeRecentProject: (path: string) => invoke<Settings>("remove_recent_project", { path }),

  listSessions: (project?: string) => invoke<SessionSummary[]>("list_sessions", { project }),
  createSession: (project: string, provider: string, model: string) =>
    invoke<Session>("create_session", { project, provider, model }),
  loadSession: (id: string) => invoke<Session>("load_session", { id }),
  deleteSession: (id: string) => invoke<void>("delete_session", { id }),
  renameSession: (id: string, title: string) => invoke<void>("rename_session", { id, title }),

  send: (sessionId: string, text: string, provider: string, model: string) =>
    invoke<void>("agent_send", { sessionId, text, provider, model }),
  stop: (sessionId: string) => invoke<void>("agent_stop", { sessionId }),
  running: () => invoke<string[]>("agent_running"),
  decide: (requestId: string, decision: Decision) => invoke<void>("agent_decide", { requestId, decision }),
  revertRun: (sessionId: string, runId: string) => invoke<string[]>("revert_run", { sessionId, runId }),

  fsList: (root: string, rel?: string) => invoke<FsEntry[]>("fs_list", { root, rel }),
  fsRead: (root: string, rel: string) => invoke<string>("fs_read", { root, rel }),

  skillsList: (project?: string) => invoke<Skill[]>("skills_list", { project }),
  skillRead: (dir: string) => invoke<string>("skill_read", { dir }),
  skillSave: (scope: string, project: string | undefined, name: string, content: string) =>
    invoke<string>("skill_save", { scope, project, name, content }),
  skillDelete: (dir: string) => invoke<void>("skill_delete", { dir }),
  skillToggle: (scope: string, name: string, enabled: boolean) => invoke<void>("skill_toggle", { scope, name, enabled }),
  skillsPaths: (project?: string) => invoke<{ global: string; project: string | null }>("skills_paths", { project }),

  ptySpawn: (cwd: string, cols: number, rows: number) => invoke<string>("pty_spawn", { cwd, cols, rows }),
  ptyWrite: (id: string, data: string) => invoke<void>("pty_write", { id, data }),
  ptyResize: (id: string, cols: number, rows: number) => invoke<void>("pty_resize", { id, cols, rows }),
  ptyKill: (id: string) => invoke<void>("pty_kill", { id }),
};

export function basename(p: string) {
  const parts = p.replace(/\\/g, "/").replace(/\/$/, "").split("/");
  return parts[parts.length - 1] || p;
}
