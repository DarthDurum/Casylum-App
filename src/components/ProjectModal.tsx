import { useState } from "react";
import { FolderOpen, Trash2, X } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, basename } from "../lib/api";
import { useStore } from "../store";
import { Modal } from "./Modal";

/** Create or edit a project: a name, an optional folder and instructions. */
export function ProjectModal() {
  const setModal = useStore((s) => s.setModal);
  const editing = useStore((s) => (s.editingProject ? s.projects.find((p) => p.id === s.editingProject) : undefined));
  const notify = useStore((s) => s.notify);
  const currentFolder = useStore((s) => s.project);
  const [name, setName] = useState(editing?.name ?? "");
  const [folder, setFolder] = useState(editing?.folder ?? currentFolder ?? "");
  const [instructions, setInstructions] = useState(editing?.instructions ?? "");
  const [busy, setBusy] = useState(false);
  const close = () => setModal(null);

  const pick = async () => {
    const d = await open({ directory: true, multiple: false });
    if (typeof d === "string") {
      setFolder(d);
      if (!name.trim()) setName(basename(d));
    }
  };

  const save = async () => {
    if (!name.trim()) return;
    setBusy(true);
    try {
      const st = useStore.getState();
      if (editing) {
        await api.projectUpdate(editing.id, name, folder, instructions);
        await st.refreshProjects();
        notify("Project saved");
      } else {
        const p = await api.projectCreate(name, folder, instructions);
        await st.refreshProjects();
        if (p.folder) useStore.setState({ settings: await api.getSettings() });
        await st.newChat(p.id);
        localStorage.setItem(`project-open:${p.id}`, "1");
      }
      close();
    } catch (e) {
      notify(String(e));
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    if (!editing) return;
    try {
      await api.projectDelete(editing.id);
      const st = useStore.getState();
      await st.refreshProjects();
      await st.refreshSessions();
      notify(`Deleted ${editing.name}. Its chats are kept.`);
      close();
    } catch (e) {
      notify(String(e));
    }
  };

  return (
    <Modal title={editing ? "Edit project" : "New project"} onClose={close}>
      <div className="form project-form">
        <label>
          <span>Name</span>
          <input className="text-input" autoFocus value={name} placeholder="e.g. Shopfront redesign" onChange={(e) => setName(e.target.value)} onKeyDown={(e) => e.key === "Enter" && save()} />
        </label>
        <div className="form-field">
          <span>
            Folder <span className="muted">· optional</span>
          </span>
          <div className="folder-row">
            <button className="btn" onClick={pick}>
              <FolderOpen size={14} /> {folder ? "Change" : "Choose folder"}
            </button>
            {folder ? (
              <>
                <span className="mono folder-path" title={folder}>
                  {folder}
                </span>
                <button className="icon-btn sm" title="No folder" onClick={() => setFolder("")}>
                  <X size={12} />
                </button>
              </>
            ) : (
              <span className="muted small">New chats in this project start without a folder.</span>
            )}
          </div>
        </div>
        <label>
          <span>
            Instructions <span className="muted">· optional</span>
          </span>
          <textarea
            className="text-input"
            rows={7}
            value={instructions}
            placeholder="What the agent should know or follow in every chat of this project. For example: the stack, coding style, who it's for."
            onChange={(e) => setInstructions(e.target.value)}
          />
        </label>
        <div className="form-actions">
          {editing && (
            <button className="btn ghost danger" onClick={remove}>
              <Trash2 size={14} /> Delete project
            </button>
          )}
          <div className="spacer" />
          <button className="btn" onClick={close}>
            Cancel
          </button>
          <button className="btn primary" onClick={save} disabled={busy || !name.trim()}>
            {editing ? "Save" : "Create project"}
          </button>
        </div>
      </div>
    </Modal>
  );
}
