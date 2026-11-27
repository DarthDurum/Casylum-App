import { create } from "zustand";
import {
  api,
  type AgentEvent,
  type Message,
  type Provider,
  type RunInfo,
  type SessionSummary,
  type Settings,
  type Usage,
} from "./lib/api";

export interface Approval {
  requestId: string;
  name: string;
  input: any;
}

export interface Draft {
  text: string;
  thinking: string;
  pendingTool: string | null;
}

export interface ChatState {
  messages: Message[];
  draft: Draft | null;
  running: boolean;
  approvals: Record<string, Approval>; // by tool_use_id
  activeTool: string | null; // tool_use_id currently executing
  liveResults: Record<string, Extract<Message["content"][number], { type: "tool_result" }>>; // results not yet committed
  usage: Usage;
  runs: RunInfo[];
  error: string | null;
  lastRun: { id: string; files: number } | null;
}

export type PanelTab = "plan" | "changes" | "files" | "terminal";
export type Modal = null | "settings" | "skills" | "palette";

const emptyChat = (): ChatState => ({
  messages: [],
  draft: null,
  running: false,
  approvals: {},
  activeTool: null,
  liveResults: {},
  usage: { input_tokens: 0, output_tokens: 0 },
  runs: [],
  error: null,
  lastRun: null,
});

interface Store {
  ready: boolean;
  settings: Settings | null;
  providers: Provider[];
  project: string | null;
  sessions: SessionSummary[];
  sessionId: string | null;
  chats: Record<string, ChatState>;
  panel: PanelTab;
  panelOpen: boolean;
  modal: Modal;
  focusFile: string | null;
  toast: string | null;

  init(): Promise<void>;
  refreshProviders(): Promise<void>;
  updateSettings(patch: Partial<Settings>): Promise<void>;
  openProject(path: string): Promise<void>;
  closeProject(path: string): Promise<void>;
  refreshSessions(): Promise<void>;
  newChat(): Promise<string | null>;
  selectSession(id: string): Promise<void>;
  deleteSession(id: string): Promise<void>;
  send(text: string): Promise<void>;
  stop(): Promise<void>;
  decide(toolUseId: string, decision: "allow" | "allow_always" | "deny"): Promise<void>;
  revert(runId: string): Promise<void>;
  setPanel(tab: PanelTab): void;
  togglePanel(): void;
  setModal(m: Modal): void;
  showFile(path: string): void;
  notify(msg: string): void;
  handleEvent(ev: AgentEvent): void;
}

let toastTimer: number | undefined;

export const useStore = create<Store>((set, get) => {
  const patchChat = (sid: string, fn: (c: ChatState) => Partial<ChatState>) =>
    set((s) => {
      const cur = s.chats[sid] ?? emptyChat();
      return { chats: { ...s.chats, [sid]: { ...cur, ...fn(cur) } } };
    });

  const loadChat = async (id: string) => {
    const sess = await api.loadSession(id);
    patchChat(id, (c) => ({ messages: sess.messages, usage: sess.usage, runs: sess.runs, running: c.running }));
  };

  return {
    ready: false,
    settings: null,
    providers: [],
    project: null,
    sessions: [],
    sessionId: null,
    chats: {},
    panel: "plan",
    panelOpen: true,
    modal: null,
    focusFile: null,
    toast: null,

    async init() {
      const [settings, providers, running] = await Promise.all([api.getSettings(), api.listProviders(), api.running()]);
      document.documentElement.dataset.theme = settings.theme;
      const project = settings.recent_projects[0] ?? null;
      set({ settings, providers, project, ready: true });
      for (const sid of running) patchChat(sid, () => ({ running: true }));
      await get().refreshSessions();
      const first = get().sessions[0];
      if (first) await get().selectSession(first.id);
    },

    async refreshProviders() {
      set({ providers: await api.listProviders() });
    },

    async updateSettings(patch) {
      const cur = get().settings;
      if (!cur) return;
      const next = { ...cur, ...patch };
      set({ settings: next });
      if (patch.theme) document.documentElement.dataset.theme = patch.theme;
      await api.saveSettings(next);
    },

    async openProject(path) {
      const settings = await api.addRecentProject(path);
      set({ settings, project: path, sessionId: null });
      await get().refreshSessions();
      const first = get().sessions[0];
      if (first) await get().selectSession(first.id);
    },

    async closeProject(path) {
      const settings = await api.removeRecentProject(path);
      const project = get().project === path ? settings.recent_projects[0] ?? null : get().project;
      set({ settings, project, sessionId: get().project === path ? null : get().sessionId });
      await get().refreshSessions();
    },

    async refreshSessions() {
      const project = get().project;
      set({ sessions: project ? await api.listSessions(project) : [] });
    },

    async newChat() {
      const { project, settings } = get();
      if (!project || !settings) return null;
      const s = await api.createSession(project, settings.provider, settings.model);
      patchChat(s.id, () => emptyChat());
      set({ sessionId: s.id });
      await get().refreshSessions();
      return s.id;
    },

    async selectSession(id) {
      set({ sessionId: id });
      await loadChat(id);
    },

    async deleteSession(id) {
      await api.deleteSession(id);
      const chats = { ...get().chats };
      delete chats[id];
      set({ chats, sessionId: get().sessionId === id ? null : get().sessionId });
      await get().refreshSessions();
    },

    async send(text) {
      const { settings } = get();
      if (!settings) return;
      let sid = get().sessionId;
      if (!sid) sid = await get().newChat();
      if (!sid) return;
      patchChat(sid, () => ({ running: true, error: null, lastRun: null, draft: { text: "", thinking: "", pendingTool: null } }));
      try {
        await api.send(sid, text, settings.provider, settings.model);
      } catch (e) {
        patchChat(sid, () => ({ running: false, draft: null, error: String(e) }));
      }
    },

    async stop() {
      const sid = get().sessionId;
      if (sid) await api.stop(sid);
    },

    async decide(toolUseId, decision) {
      const sid = get().sessionId;
      if (!sid) return;
      const ap = get().chats[sid]?.approvals[toolUseId];
      if (!ap) return;
      patchChat(sid, (c) => {
        const approvals = { ...c.approvals };
        delete approvals[toolUseId];
        return { approvals };
      });
      await api.decide(ap.requestId, decision);
    },

    async revert(runId) {
      const sid = get().sessionId;
      if (!sid) return;
      try {
        const files = await api.revertRun(sid, runId);
        get().notify(`Undid changes to ${files.length} file${files.length === 1 ? "" : "s"}`);
        await loadChat(sid);
        patchChat(sid, (c) => ({ lastRun: c.lastRun?.id === runId ? null : c.lastRun }));
      } catch (e) {
        get().notify(String(e));
      }
    },

    setPanel(tab) {
      set({ panel: tab, panelOpen: true });
    },
    togglePanel() {
      set({ panelOpen: !get().panelOpen });
    },
    setModal(m) {
      set({ modal: m });
    },
    showFile(path) {
      set({ focusFile: path, panel: "changes", panelOpen: true });
    },
    notify(msg) {
      set({ toast: msg });
      window.clearTimeout(toastTimer);
      toastTimer = window.setTimeout(() => set({ toast: null }), 3200);
    },

    handleEvent(ev) {
      const sid = ev.session_id;
      switch (ev.kind) {
        case "started":
          patchChat(sid, () => ({ running: true, error: null, draft: { text: "", thinking: "", pendingTool: null } }));
          break;
        case "text_delta":
          patchChat(sid, (c) => ({ draft: { ...(c.draft ?? { text: "", thinking: "", pendingTool: null }), text: (c.draft?.text ?? "") + ev.text } }));
          break;
        case "thinking_delta":
          patchChat(sid, (c) => ({ draft: { ...(c.draft ?? { text: "", thinking: "", pendingTool: null }), thinking: (c.draft?.thinking ?? "") + ev.text } }));
          break;
        case "tool_started":
          patchChat(sid, (c) => ({ draft: { ...(c.draft ?? { text: "", thinking: "", pendingTool: null }), pendingTool: ev.name } }));
          break;
        case "message":
          patchChat(sid, (c) => ({
            messages: [...c.messages, ev.message],
            draft: ev.message.role === "assistant" ? null : c.running ? { text: "", thinking: "", pendingTool: null } : null,
            activeTool: null,
            liveResults: ev.message.role === "user" ? {} : c.liveResults,
          }));
          if (ev.message.role === "user" && ev.message.content.some((b) => b.type === "text")) void get().refreshSessions();
          break;
        case "tool_result":
          patchChat(sid, (c) => ({ liveResults: { ...c.liveResults, [ev.block.tool_use_id]: ev.block }, activeTool: null }));
          break;
        case "tool_running":
          patchChat(sid, () => ({ activeTool: ev.tool_use_id }));
          break;
        case "approval":
          patchChat(sid, (c) => ({
            approvals: { ...c.approvals, [ev.tool_use_id]: { requestId: ev.request_id, name: ev.name, input: ev.input } },
          }));
          break;
        case "usage":
          patchChat(sid, () => ({ usage: ev.total }));
          break;
        case "error":
          patchChat(sid, () => ({ error: ev.message }));
          break;
        case "done":
          patchChat(sid, () => ({
            running: false,
            draft: null,
            approvals: {},
            activeTool: null,
            liveResults: {},
            lastRun: ev.files_changed > 0 ? { id: ev.run_id, files: ev.files_changed } : null,
          }));
          void get().refreshSessions();
          if (get().sessionId === sid) void loadChat(sid);
          break;
      }
    },
  };
});

export const useChat = () =>
  useStore((s) => (s.sessionId ? s.chats[s.sessionId] : undefined) ?? EMPTY);
const EMPTY = emptyChat();

export const currentProvider = (s: { settings: Settings | null; providers: Provider[] }) =>
  s.providers.find((p) => p.id === s.settings?.provider);
