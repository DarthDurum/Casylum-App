import { useState } from "react";
import { ChevronDown, Command, FolderOpen, Moon, Plus, Settings2, Sparkles, Sun, Trash2, X } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { basename } from "../lib/api";
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
  const [picker, setPicker] = useState(false);

  const pickFolder = async () => {
    const dir = await open({ directory: true, multiple: false });
    if (typeof dir === "string") await st().openProject(dir);
    setPicker(false);
  };

  return (
    <aside className="rail">
      <div className="brand">
        <div className="brand-mark" />
        <span>Casylum</span>
      </div>

      <div className="project-switch">
        <button className="project-btn" onClick={() => setPicker((p) => !p)}>
          <FolderOpen size={14} />
          <span className="project-name">{project ? basename(project) : "Open a project"}</span>
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
          </div>
        )}
      </div>

      <button className="new-chat" onClick={() => st().newChat()} disabled={!project}>
        <Plus size={16} />
        New chat
        <kbd>Ctrl N</kbd>
      </button>
      <button className="new-chat" onClick={() => st().setModal("palette")}>
        <Command size={15} />
        Commands
        <kbd>Ctrl K</kbd>
      </button>

      <div className="rail-label">Chats</div>
      <nav className="session-list">
        {sessions.length === 0 && <div className="rail-empty">No chats yet</div>}
        {sessions.map((s) => (
          <div key={s.id} className={`session ${s.id === sessionId ? "active" : ""}`} onClick={() => st().selectSession(s.id)}>
            {chats[s.id]?.running && <span className="live-dot" title="Working" />}
            <span className="session-title">{s.title}</span>
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
        ))}
      </nav>

      <div className="rail-foot">
        <button className="rail-btn" onClick={() => st().setModal("skills")}>
          <Sparkles size={15} /> Skills
        </button>
        <button className="rail-btn" onClick={() => st().setModal("settings")}>
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
