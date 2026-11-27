import { useEffect, useState } from "react";
import { FolderGit2, Globe, Plus, Sparkles, Trash2 } from "lucide-react";
import { api, type Skill } from "../lib/api";
import { useStore } from "../store";
import { Modal } from "./Modal";
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

export function SkillsModal() {
  const setModal = useStore((s) => s.setModal);
  const project = useStore((s) => s.project) ?? undefined;
  const notify = useStore((s) => s.notify);
  const [skills, setSkills] = useState<Skill[]>([]);
  const [sel, setSel] = useState<Skill | null>(null);
  const [draft, setDraft] = useState<{ scope: "global" | "project"; name: string; content: string; isNew: boolean } | null>(null);
  const [paths, setPaths] = useState<{ global: string; project: string | null }>({ global: "", project: null });

  const load = async () => {
    setSkills(await api.skillsList(project));
    setPaths(await api.skillsPaths(project));
  };
  useEffect(() => {
    void load();
  }, [project]);

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

  const groups: Array<["project" | "global", string]> = [
    ["project", "This project"],
    ["global", "All projects"],
  ];

  return (
    <Modal title="Skills" onClose={() => setModal(null)} wide>
      <div className="skills">
        <div className="skills-side">
          <p className="muted small">Skills are reusable instructions. The agent sees each skill's description and loads the full text when a task needs it.</p>
          <div className="skills-new">
            <button
              className="btn sm"
              disabled={!project}
              onClick={() => {
                setSel(null);
                setDraft({ scope: "project", name: "my-skill", content: TEMPLATE("my-skill"), isNew: true });
              }}
            >
              <Plus size={13} /> Project skill
            </button>
            <button
              className="btn sm"
              onClick={() => {
                setSel(null);
                setDraft({ scope: "global", name: "my-skill", content: TEMPLATE("my-skill"), isNew: true });
              }}
            >
              <Plus size={13} /> Global skill
            </button>
          </div>
          {groups.map(([scope, label]) => {
            const list = skills.filter((s) => s.scope === scope);
            return (
              <div key={scope} className="skill-group">
                <div className="menu-label">
                  {scope === "project" ? <FolderGit2 size={12} /> : <Globe size={12} />} {label}
                </div>
                {list.length === 0 && <div className="rail-empty">None yet</div>}
                {list.map((s) => (
                  <div key={s.dir} className={`skill-row ${sel?.dir === s.dir ? "on" : ""}`} onClick={() => openSkill(s)}>
                    <Sparkles size={13} />
                    <div className="skill-meta">
                      <span>{s.name}</span>
                      <small>{s.description || "No description"}</small>
                    </div>
                    <label className="switch" onClick={(e) => e.stopPropagation()}>
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
            <div>Global: <span className="mono">{paths.global}</span></div>
            {paths.project && <div>Project: <span className="mono">{paths.project}</span></div>}
          </div>
        </div>
        <div className="skills-editor">
          {draft ? (
            <>
              <div className="skill-editor-head">
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
                <span className="tag">{draft.scope === "project" ? "Project" : "Global"}</span>
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
            </>
          ) : (
            <div className="panel-empty">
              <Sparkles size={22} />
              <p>Pick a skill to edit, or create one.</p>
              <small>A skill is a folder with a SKILL.md. Drop existing Claude Code-style skill folders into the paths on the left and they just work.</small>
            </div>
          )}
        </div>
      </div>
    </Modal>
  );
}
