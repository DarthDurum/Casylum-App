import { useEffect, useMemo, useRef, useState } from "react";
import { ChevronRight, Circle, CircleCheck, CircleDot, File, Folder, RefreshCw, RotateCcw } from "lucide-react";
import { api, type FsEntry } from "../lib/api";
import { latestPlan, sessionChanges } from "../lib/steps";
import { useChat, useStore, type PanelTab } from "../store";
import { CodeView, DiffView } from "./Code";
import { TerminalView } from "./Terminal";

const TABS: Array<{ id: PanelTab; label: string }> = [
  { id: "plan", label: "Plan" },
  { id: "changes", label: "Changes" },
  { id: "files", label: "Files" },
  { id: "terminal", label: "Terminal" },
];

export function ContextPanel() {
  const panel = useStore((s) => s.panel);
  const setPanel = useStore((s) => s.setPanel);
  const project = useStore((s) => s.project);
  const chat = useChat();
  const changes = useMemo(() => sessionChanges(chat.messages), [chat.messages]);
  const [width, setWidth] = useState(() => Number(localStorage.getItem("panelWidth")) || 440);
  const [termMounted, setTermMounted] = useState(false);

  useEffect(() => {
    if (panel === "terminal") setTermMounted(true);
  }, [panel]);

  const startDrag = (e: React.MouseEvent) => {
    const startX = e.clientX;
    const startW = width;
    const move = (ev: MouseEvent) => {
      const w = Math.min(Math.max(startW - (ev.clientX - startX), 320), Math.min(900, window.innerWidth - 560));
      setWidth(w);
      localStorage.setItem("panelWidth", String(w));
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      document.body.classList.remove("dragging");
    };
    document.body.classList.add("dragging");
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  return (
    <aside className="panel" style={{ width }}>
      <div className="panel-drag" onMouseDown={startDrag} />
      <div className="tabs">
        {TABS.map((t) => (
          <button key={t.id} className={`tab ${panel === t.id ? "on" : ""}`} onClick={() => setPanel(t.id)}>
            {t.label}
            {t.id === "changes" && changes.length > 0 && <span className="count">{changes.length}</span>}
          </button>
        ))}
      </div>
      <div className="panel-body">
        {panel === "plan" && <PlanTab />}
        {panel === "changes" && <ChangesTab />}
        {panel === "files" && project && <FilesTab root={project} />}
        {project && termMounted && (
          <div className="term-slot" style={{ display: panel === "terminal" ? "flex" : "none" }}>
            <TerminalView key={project} cwd={project} visible={panel === "terminal"} />
          </div>
        )}
        {!project && panel !== "plan" && <div className="panel-empty">Open a folder to see files and use the terminal.</div>}
      </div>
    </aside>
  );
}

function PlanTab() {
  const chat = useChat();
  const plan = useMemo(() => latestPlan(chat.messages), [chat.messages]);
  if (!plan.length)
    return (
      <div className="panel-empty">
        <p>No plan yet.</p>
        <small>For multi-step work, Casylum writes its plan here and checks items off as it goes.</small>
      </div>
    );
  const done = plan.filter((p) => p.status === "done").length;
  return (
    <div className="plan">
      <div className="plan-progress">
        <div className="bar">
          <div style={{ width: `${(done / plan.length) * 100}%` }} />
        </div>
        <span>
          {done} of {plan.length}
        </span>
      </div>
      {plan.map((p, i) => (
        <div key={i} className={`plan-item ${p.status}`}>
          {p.status === "done" ? <CircleCheck size={15} /> : p.status === "in_progress" ? <CircleDot size={15} /> : <Circle size={15} />}
          <div className="plan-text">
            <span className="plan-title">{p.text}</span>
            {typeof p.description === "string" && p.description.trim() && <span className="plan-desc">{p.description}</span>}
          </div>
        </div>
      ))}
    </div>
  );
}

function ChangesTab() {
  const chat = useChat();
  const focusFile = useStore((s) => s.focusFile);
  const revert = useStore((s) => s.revert);
  const changes = useMemo(() => sessionChanges(chat.messages), [chat.messages]);
  const [sel, setSel] = useState<string | null>(null);
  useEffect(() => {
    if (focusFile) setSel(focusFile);
  }, [focusFile]);
  const current = changes.find((c) => c.path === sel) ?? changes[0];
  const runs = chat.runs.filter((r) => !r.reverted);

  if (!changes.length)
    return (
      <div className="panel-empty">
        <p>No changes yet.</p>
        <small>Every file Casylum edits in this chat shows up here as a diff.</small>
      </div>
    );

  return (
    <div className="changes">
      <div className="change-list">
        {changes.map((c) => (
          <button key={c.path} className={`change ${current?.path === c.path ? "on" : ""}`} onClick={() => setSel(c.path)}>
            <span className={`badge ${c.created ? "new" : "mod"}`}>{c.created ? "A" : "M"}</span>
            <span className="change-path">{c.path}</span>
            <span className="diffstat">
              <em>+{c.added}</em> <i>−{c.removed}</i>
            </span>
          </button>
        ))}
      </div>
      {current && (
        <div className="diff-pane">
          {current.after == null ? (
            <div className="panel-empty">This file is too large to show a diff.</div>
          ) : (
            <DiffView path={current.path} before={current.before ?? ""} after={current.after} />
          )}
        </div>
      )}
      {runs.length > 0 && (
        <div className="undo-bar">
          <span>
            {runs.length} run{runs.length === 1 ? "" : "s"} with changes
          </span>
          <button className="btn ghost sm" onClick={() => revert(runs[runs.length - 1].id)}>
            <RotateCcw size={13} /> Undo latest run
          </button>
        </div>
      )}
    </div>
  );
}

function FilesTab({ root }: { root: string }) {
  const [open, setOpen] = useState<string | null>(null);
  const [content, setContent] = useState<string>("");
  const [err, setErr] = useState<string | null>(null);
  const [rev, setRev] = useState(0);

  const view = async (path: string) => {
    setOpen(path);
    setErr(null);
    try {
      setContent(await api.fsRead(root, path));
    } catch (e) {
      setErr(String(e));
      setContent("");
    }
  };

  return (
    <div className="files">
      <div className="tree">
        <div className="tree-head">
          <span>Explorer</span>
          <button className="icon-btn sm" title="Refresh" onClick={() => setRev((r) => r + 1)}>
            <RefreshCw size={12} />
          </button>
        </div>
        <TreeLevel key={rev} root={root} rel="." depth={0} onOpen={view} selected={open} />
      </div>
      <div className="file-view">
        {open ? (
          <>
            <div className="file-path">{open}</div>
            {err ? <div className="panel-empty">{err}</div> : <CodeView path={open} value={content} />}
          </>
        ) : (
          <div className="panel-empty">Pick a file to preview it.</div>
        )}
      </div>
    </div>
  );
}

function TreeLevel({ root, rel, depth, onOpen, selected }: { root: string; rel: string; depth: number; onOpen: (p: string) => void; selected: string | null }) {
  const [items, setItems] = useState<FsEntry[] | null>(null);
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    api.fsList(root, rel).then((r) => mounted.current && setItems(r)).catch(() => setItems([]));
    return () => {
      mounted.current = false;
    };
  }, [root, rel]);
  if (!items) return <div className="tree-loading" style={{ paddingLeft: 12 + depth * 14 }}>…</div>;
  return (
    <>
      {items.map((it) => (
        <div key={it.path}>
          <button
            className={`tree-row ${selected === it.path ? "on" : ""}`}
            style={{ paddingLeft: 10 + depth * 14 }}
            onClick={() => (it.is_dir ? setExpanded((e) => ({ ...e, [it.path]: !e[it.path] })) : onOpen(it.path))}
          >
            {it.is_dir ? <ChevronRight size={12} className={expanded[it.path] ? "rot90" : ""} /> : <span style={{ width: 12 }} />}
            {it.is_dir ? <Folder size={13} /> : <File size={13} />}
            <span>{it.name}</span>
          </button>
          {it.is_dir && expanded[it.path] && <TreeLevel root={root} rel={it.path} depth={depth + 1} onOpen={onOpen} selected={selected} />}
        </div>
      ))}
    </>
  );
}
