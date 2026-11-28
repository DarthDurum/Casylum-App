import { useEffect, useMemo, useRef, useState } from "react";
import { Search } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { useStore } from "../store";
import { modelLabel } from "../lib/models";

interface Cmd {
  id: string;
  label: string;
  hint?: string;
  run: () => void | Promise<void>;
}

export function Palette() {
  const st = useStore.getState;
  const providers = useStore((s) => s.providers);
  const models = useStore((s) => s.models);
  const sessions = useStore((s) => s.sessions);
  const projects = useStore((s) => s.projects);
  const [q, setQ] = useState("");
  const [i, setI] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => input.current?.focus(), []);

  const cmds = useMemo<Cmd[]>(() => {
    const base: Cmd[] = [
      { id: "new", label: "New chat", hint: "Ctrl N", run: () => void st().newChat() },
      {
        id: "open",
        label: "Open project folder…",
        hint: "Ctrl O",
        run: async () => {
          const d = await open({ directory: true });
          if (typeof d === "string") await st().openProject(d);
        },
      },
      { id: "settings", label: "Settings: providers and keys", hint: "Ctrl ,", run: () => st().openSettings("providers") },
      { id: "skills", label: "Skills: manage", run: () => st().openSettings("skills") },
      { id: "mcp", label: "MCP servers: manage", run: () => st().openSettings("mcp") },
      { id: "new-project", label: "New project…", run: () => st().editProject(null) },
      { id: "theme", label: "Toggle light / dark theme", run: () => void st().updateSettings({ theme: st().settings?.theme === "light" ? "dark" : "light" }) },
      { id: "panel", label: "Toggle side panel", hint: "Ctrl \\", run: () => st().togglePanel() },
      { id: "plan", label: "Show plan", run: () => st().setPanel("plan") },
      { id: "changes", label: "Show changes", run: () => st().setPanel("changes") },
      { id: "files", label: "Show files", run: () => st().setPanel("files") },
      { id: "term", label: "Show terminal", hint: "Ctrl `", run: () => st().setPanel("terminal") },
      { id: "mode-ask", label: "Permissions: ask first", run: () => void st().updateSettings({ permission_mode: "ask" }) },
      { id: "mode-auto", label: "Permissions: auto-edit", run: () => void st().updateSettings({ permission_mode: "auto_edit" }) },
      { id: "mode-full", label: "Permissions: full auto", run: () => void st().updateSettings({ permission_mode: "full_auto" }) },
    ];
    for (const p of providers)
      if (p.has_key || !p.needs_key)
        for (const m of models[p.id]?.models ?? p.models.map((id) => ({ id }))) {
          const all = models[p.id]?.models;
          base.push({ id: `m-${p.id}-${m.id}`, label: `Model: ${modelLabel(m.id, all)}`, hint: `${p.name} · ${m.id}`, run: () => void st().updateSettings({ provider: p.id, model: m.id }) });
        }
    for (const p of projects) base.push({ id: `p-${p.id}`, label: `New chat in ${p.name}`, hint: "Project", run: () => void st().newChat(p.id) });
    for (const s of sessions.slice(0, 30)) base.push({ id: `s-${s.id}`, label: `Chat: ${s.title}`, run: () => void st().selectSession(s.id) });
    return base;
  }, [providers, sessions, models, projects]);

  const filtered = useMemo(() => {
    const words = q.toLowerCase().split(/\s+/).filter(Boolean);
    return cmds.filter((c) => words.every((w) => (c.label + " " + (c.hint ?? "")).toLowerCase().includes(w))).slice(0, 40);
  }, [q, cmds]);

  const run = (c?: Cmd) => {
    if (!c) return;
    st().setModal(null);
    void c.run();
  };

  return (
    <div className="overlay top" onMouseDown={() => st().setModal(null)}>
      <div className="palette" onMouseDown={(e) => e.stopPropagation()}>
        <div className="palette-input">
          <Search size={15} />
          <input
            ref={input}
            value={q}
            placeholder="Type a command, model or chat…"
            onChange={(e) => {
              setQ(e.target.value);
              setI(0);
            }}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") (e.preventDefault(), setI((x) => Math.min(x + 1, filtered.length - 1)));
              else if (e.key === "ArrowUp") (e.preventDefault(), setI((x) => Math.max(x - 1, 0)));
              else if (e.key === "Enter") run(filtered[i]);
              else if (e.key === "Escape") st().setModal(null);
            }}
          />
        </div>
        <div className="palette-list">
          {filtered.map((c, idx) => (
            <button key={c.id} className={`palette-item ${idx === i ? "on" : ""}`} onMouseEnter={() => setI(idx)} onClick={() => run(c)}>
              <span>{c.label}</span>
              {c.hint && <kbd>{c.hint}</kbd>}
            </button>
          ))}
          {filtered.length === 0 && <div className="menu-note">No matches</div>}
        </div>
      </div>
    </div>
  );
}
