import { useEffect, useRef, useState } from "react";
import { ArrowLeft, FileArchive, FolderGit2, Globe, Plus, Sparkles, Trash2 } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, type Skill } from "../lib/api";
import { useStore } from "../store";
import { CodeView } from "./Code";

const TEMPLATE = (name: string) => `---
name: ${name}
description: One sentence on when the agent should use this skill.
---

# ${name}

Step-by-step instructions the agent follows when this skill applies.

1. ...
2. ...
`;

type Scope = "global" | "project";

/** A button with a small menu: "All projects" or "This folder". */
function ScopeButton({ icon, label, onPick, hasFolder }: { icon: React.ReactNode; label: string; onPick: (s: Scope) => void; hasFolder: boolean }) {
  const [menu, setMenu] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!menu) return;
    const close = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && setMenu(false);
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [menu]);
  return (
    <div className="menu-anchor" ref={ref}>
      <button className="btn sm" onClick={() => (hasFolder ? setMenu((m) => !m) : onPick("global"))}>
        {icon} {label}
      </button>
      {menu && (
        <div className="menu down scope-menu">
          <button className="menu-item" onClick={() => (setMenu(false), onPick("global"))}>
            <Globe size={14} /> For all chats
          </button>
          <button className="menu-item" onClick={() => (setMenu(false), onPick("project"))}>
            <FolderGit2 size={14} /> Only this chat's folder
          </button>
        </div>
      )}
    </div>
  );
}

/** Skills page inside Settings: a list, and an editor for one skill. */
export function SkillsPane({ onEditing }: { onEditing: (editing: boolean) => void }) {
  const project = useStore((s) => s.project) ?? undefined;
  const notify = useStore((s) => s.notify);
  const [skills, setSkills] = useState<Skill[]>([]);
  const [sel, setSel] = useState<Skill | null>(null);
  const [draft, setDraft] = useState<{ scope: Scope; name: string; content: string; isNew: boolean } | null>(null);
  const [paths, setPaths] = useState<{ global: string; project: string | null }>({ global: "", project: null });

  const load = async () => {
    setSkills(await api.skillsList(project));
    setPaths(await api.skillsPaths(project));
  };
  useEffect(() => {
    void load();
  }, [project]);
  useEffect(() => onEditing(!!draft), [draft]);

  const openSkill = async (s: Skill) => {
    setSel(s);
    setDraft({ scope: s.scope, name: s.name, content: await api.skillRead(s.dir), isNew: false });
  };

  const save = async () => {
    if (!draft) return;
    try {
      const folder = draft.isNew ? draft.name : sel ? sel.dir.replace(/\\/g, "/").split("/").pop()! : draft.name;
      await api.skillSave(draft.scope, project, folder, draft.content);
      notify("Skill saved");
      setDraft((d) => (d ? { ...d, isNew: false } : d));
      await load();
    } catch (e) {
      notify(String(e));
    }
  };

  const importZip = async (scope: Scope) => {
    const file = await open({ multiple: false, directory: false, filters: [{ name: "Skill package", extensions: ["zip", "skill"] }] });
    if (typeof file !== "string") return;
    try {
      const got = await api.skillImportZip(file, scope, project);
      const names = got.map((g) => g.name).join(", ");
      const replaced = got.filter((g) => g.replaced).length;
      notify(`Imported ${got.length === 1 ? "skill" : `${got.length} skills`}: ${names}${replaced ? ` (${replaced} replaced)` : ""}`);
      await load();
    } catch (e) {
      notify(String(e));
    }
  };

  if (draft)
    return (
      <div className="skill-edit">
        <div className="skill-editor-head">
          <button
            className="icon-btn"
            title="Back to skills"
            onClick={() => {
              setDraft(null);
              setSel(null);
            }}
          >
            <ArrowLeft size={15} />
          </button>
          {draft.isNew ? (
            <input
              className="name-input"
              value={draft.name}
              onChange={(e) => {
                const name = e.target.value.replace(/[^A-Za-z0-9_-]/g, "-");
                setDraft({ ...draft, name, content: draft.content.replace(/^name: .*$/m, `name: ${name}`) });
              }}
            />
          ) : (
            <span className="skill-title">{draft.name}</span>
          )}
          <span className="tag">{draft.scope === "project" ? "This folder" : "All chats"}</span>
          <div className="spacer" />
          {sel && (
            <button
              className="icon-btn"
              title="Delete skill"
              onClick={async () => {
                await api.skillDelete(sel.dir);
                setSel(null);
                setDraft(null);
                await load();
              }}
            >
              <Trash2 size={14} />
            </button>
          )}
          <button className="btn primary sm" onClick={save}>
            Save
          </button>
        </div>
        <div className="skill-code">
          <CodeView key={(sel?.dir ?? "new") + draft.scope} path="SKILL.md" value={draft.content} onChange={(content) => setDraft((d) => (d ? { ...d, content } : d))} />
        </div>
      </div>
    );

  const groups: Array<[Scope, string]> = [
    ["project", "This chat's folder"],
    ["global", "All chats"],
  ];

  return (
    <>
      <div className="pane-head">
        <p className="muted small">
          Skills are reusable instructions. The agent sees each skill's description and loads the full text when a task needs it. Claude Code-style skill folders (a SKILL.md, plus any scripts) work as they are.
        </p>
        <div className="pane-actions">
          <ScopeButton
            icon={<Plus size={13} />}
            label="New skill"
            hasFolder={!!project}
            onPick={(scope) => {
              setSel(null);
              setDraft({ scope, name: "my-skill", content: TEMPLATE("my-skill"), isNew: true });
            }}
          />
          <ScopeButton icon={<FileArchive size={13} />} label="Import .zip" hasFolder={!!project} onPick={importZip} />
        </div>
      </div>
      {groups.map(([scope, label]) => {
        const list = skills.filter((s) => s.scope === scope);
        if (scope === "project" && !project) return null;
        return (
          <div key={scope} className="skill-group">
            <div className="menu-label">
              {scope === "project" ? <FolderGit2 size={12} /> : <Globe size={12} />} {label}
            </div>
            {list.length === 0 && <div className="rail-empty">None yet</div>}
            {list.map((s) => (
              <div key={s.dir} className="skill-row" onClick={() => openSkill(s)}>
                <Sparkles size={13} />
                <div className="skill-meta">
                  <span>{s.name}</span>
                  <small>{s.description || "No description"}</small>
                </div>
                <label className="switch" onClick={(e) => e.stopPropagation()} title={s.enabled ? "On" : "Off"}>
                  <input
                    type="checkbox"
                    checked={s.enabled}
                    onChange={async (e) => {
                      await api.skillToggle(s.scope, s.name, e.target.checked);
                      await load();
                    }}
                  />
                  <span />
                </label>
              </div>
            ))}
          </div>
        );
      })}
      <div className="skill-paths muted small">
        <div>
          All chats: <span className="mono">{paths.global}</span>
        </div>
        {paths.project && (
          <div>
            This folder: <span className="mono">{paths.project}</span>
          </div>
        )}
      </div>
    </>
  );
}
