import { useState } from "react";
import { Check, ChevronDown, ChevronRight, Command, Folder, FolderOpen, MessageSquare, Moon, Pencil, Plug, Plus, Settings2, Sparkles, Sun, Trash2, X } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { basename, type SessionSummary } from "../lib/api";
import { useStore } from "../store";

function ago(ms: number) {
  const s = (Date.now() - ms) / 1000;
  if (s < 60) return "now";
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86400)}d`;
}

export function Rail() {
  const settings = useStore((s) => s.settings);
  const project = useStore((s) => s.project);
  const sessions = useStore((s) => s.sessions);
  const sessionId = useStore((s) => s.sessionId);
  const chats = useStore((s) => s.chats);
  const st = useStore.getState;
  const projects = useStore((s) => s.projects);
  const mcp = useStore((s) => s.mcpServers);
  const [picker, setPicker] = useState(false);
  const known = new Set(projects.map((p) => p.id));
  // Chats outside any project (or in one that was deleted).
  const loose = sessions.filter((s) => !s.project_id || !known.has(s.project_id));
  // Folder tags only help when chats are spread over more than one folder.
  const mixedFolders = new Set(loose.map((s) => s.project)).size > 1;
  const [openProjects, setOpenProjects] = useState<Record<string, boolean>>(() => {
    const out: Record<string, boolean> = {};
    try {
      for (let i = 0; i < localStorage.length; i++) {
        const k = localStorage.key(i);
        if (k?.startsWith("project-open:")) out[k.slice(13)] = localStorage.getItem(k) === "1";
      }
    } catch {
      /* storage unavailable */
    }
    return out;
  });
  const setOpen = (id: string, v: boolean) => {
    setOpenProjects((o) => ({ ...o, [id]: v }));
    try {
      localStorage.setItem(`project-open:${id}`, v ? "1" : "0");
    } catch {
      /* storage unavailable */
    }
  };
  // The chat you're in stays visible until you fold its project.
  const current = sessions.find((s) => s.id === sessionId);
  const toggleProject = (id: string) => setOpen(id, !(openProjects[id] ?? current?.project_id === id));
  const mcpLive = mcp.filter((m) => m.status === "connected").length;
  const mcpErrors = mcp.filter((m) => m.status === "error").length;
  const mcpTitle = mcp.length ? `${mcpLive} of ${mcp.length} MCP servers connected${mcpErrors ? `, ${mcpErrors} failed` : ""}` : "Connect MCP servers";

  const pickFolder = async () => {
    const dir = await open({ directory: true, multiple: false });
    if (typeof dir === "string") await st().openProject(dir);
    setPicker(false);
  };

  return (
    <aside className="rail">
      <div className="brand" data-tauri-drag-region="deep">
        <div className="brand-mark" />
        <span>Casylum</span>
      </div>

      <div className="project-switch">
        <button className="project-btn" onClick={() => setPicker((p) => !p)} title="Folder for this chat. Other chats keep their own.">
          <FolderOpen size={14} />
          <span className="project-name">{project ? basename(project) : "No folder"}</span>
          <ChevronDown size={13} />
        </button>
        {picker && (
          <div className="menu down">
            {settings?.recent_projects.map((p) => (
              <div key={p} className="menu-item row">
                <button
                  className="mi-main"
                  onClick={() => {
                    void st().openProject(p);
                    setPicker(false);
                  }}
                >
                  {basename(p)}
                  <small>{p}</small>
                </button>
                <button className="icon-btn sm" title="Remove from list" onClick={() => st().closeProject(p)}>
                  <X size={12} />
                </button>
              </div>
            ))}
            <button className="menu-item subtle" onClick={pickFolder}>
              <FolderOpen size={14} /> Open folder…
            </button>
            <button
              className="menu-item subtle"
              onClick={() => {
                void st().leaveProject();
                setPicker(false);
              }}
            >
              <MessageSquare size={14} /> No folder
              {!project && <Check size={14} />}
            </button>
          </div>
        )}
      </div>

      <button className="new-chat" onClick={() => st().newChat()}>
        <Plus size={16} />
        New chat
        <kbd>Ctrl N</kbd>
      </button>
      <button className="new-chat" onClick={() => st().setModal("palette")}>
        <Command size={15} />
        Commands
        <kbd>Ctrl K</kbd>
      </button>

      <nav className="session-list">
        <div className="rail-label row">
          Projects
          <button className="icon-btn xs" title="New project" onClick={() => st().editProject(null)}>
            <Plus size={13} />
          </button>
        </div>
        {projects.length === 0 && (
          <button className="rail-empty link" onClick={() => st().editProject(null)}>
            Group chats with a folder and instructions
          </button>
        )}
        {projects.map((p) => {
          const list = sessions.filter((s) => s.project_id === p.id);
          const isOpen = openProjects[p.id] ?? current?.project_id === p.id;
          return (
            <div key={p.id} className="rail-project">
              <div className="project-row" onClick={() => toggleProject(p.id)} title={p.folder || p.name}>
                {isOpen ? <ChevronDown size={13} /> : <ChevronRight size={13} />}
                <Folder size={14} className="project-icon" />
                <span className="session-title">{p.name}</span>
                {list.some((s) => chats[s.id]?.running) && !isOpen && <span className="live-dot" title="Working" />}
                <button
                  className="icon-btn sm row-action"
                  title="Edit project"
                  onClick={(e) => {
                    e.stopPropagation();
                    st().editProject(p.id);
                  }}
                >
                  <Pencil size={12} />
                </button>
                <button
                  className="icon-btn sm row-action"
                  title={`New chat in ${p.name}`}
                  onClick={(e) => {
                    e.stopPropagation();
                    setOpen(p.id, true);
                    void st().newChat(p.id);
                  }}
                >
                  <Plus size={13} />
                </button>
              </div>
              {isOpen && (
                <div className="project-chats">
                  {list.length === 0 && <div className="rail-empty">No chats yet</div>}
                  {list.map((s) => (
                    <SessionRow key={s.id} s={s} active={s.id === sessionId} running={!!chats[s.id]?.running} />
                  ))}
                </div>
              )}
            </div>
          );
        })}

        <div className="rail-label">Chats</div>
        {loose.length === 0 && <div className="rail-empty">No chats yet</div>}
        {loose.map((s) => (
          <SessionRow key={s.id} s={s} active={s.id === sessionId} running={!!chats[s.id]?.running} folderTag={mixedFolders} />
        ))}
      </nav>

      <div className="rail-foot">
        <button className="rail-btn" onClick={() => st().openSettings("skills")}>
          <Sparkles size={15} /> Skills
        </button>
        <button className="rail-btn" onClick={() => st().openSettings("mcp")} title={mcpTitle}>
          <Plug size={15} /> MCP
          {(mcpLive > 0 || mcpErrors > 0) && <span className={`rail-dot ${mcpErrors ? "err" : "ok"}`} />}
        </button>
        <button className="rail-btn" onClick={() => st().openSettings("providers")} title="Settings (Ctrl+,)">
          <Settings2 size={15} /> Settings
        </button>
        <button
          className="icon-btn"
          title="Toggle theme"
          onClick={() => st().updateSettings({ theme: settings?.theme === "light" ? "dark" : "light" })}
        >
          {settings?.theme === "light" ? <Moon size={15} /> : <Sun size={15} />}
        </button>
      </div>
    </aside>
  );
}

function SessionRow({ s, active, running, folderTag }: { s: SessionSummary; active: boolean; running: boolean; folderTag?: boolean }) {
  const st = useStore.getState;
  return (
    <div className={`session ${active ? "active" : ""}`} onClick={() => st().selectSession(s.id)}>
      {running && <span className="live-dot" title="Working" />}
      <span className="session-title">{s.title}</span>
      {folderTag && s.project && (
        <span className="session-folder" title={s.project}>
          {basename(s.project)}
        </span>
      )}
      <span className="session-time">{ago(s.updated_at)}</span>
      <button
        className="icon-btn sm session-del"
        title="Delete chat"
        onClick={(e) => {
          e.stopPropagation();
          void st().deleteSession(s.id);
        }}
      >
        <Trash2 size={12} />
      </button>
    </div>
  );
}
