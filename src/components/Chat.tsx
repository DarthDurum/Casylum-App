import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import { Brain, Check, ChevronDown, Copy, FolderOpen, PanelRight, Paperclip, Pencil, RotateCcw, RotateCw, Trash2, TriangleAlert } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import type { Attachment, Message } from "../lib/api";
import { basename } from "../lib/api";
import { resultIndex, formatTokens, type ToolUse } from "../lib/steps";
import { hasReply, promptParts, replyText, turns, type Turn } from "../lib/turns";
import { AttachmentRow, readPaths, SentAttachments } from "./Attachments";
import { useChat, useStore } from "../store";
import { Markdown } from "./Markdown";
import { StepCard } from "./StepCard";
import { Composer } from "./Composer";

export function Chat() {
  const project = useStore((s) => s.project);
  const sessions = useStore((s) => s.sessions);
  const sessionId = useStore((s) => s.sessionId);
  const panelOpen = useStore((s) => s.panelOpen);
  const togglePanel = useStore((s) => s.togglePanel);
  const chat = useChat();
  const title = sessions.find((s) => s.id === sessionId)?.title ?? "New chat";

  return (
    <main className="chat">
      <header className="chat-head" data-tauri-drag-region="deep">
        <div className="chat-title">
          <ProjectCrumb />
          <span className="crumb">{project ? basename(project) : "No folder"}</span>
          <span className="crumb-sep">/</span>
          <span className="title-text">{sessionId ? title : "New chat"}</span>
        </div>
        <div className="head-right">
          {chat.usage.input_tokens + chat.usage.output_tokens > 0 && (
            <span className="usage" title="Tokens used in this chat (input / output)">
              {formatTokens(chat.usage.input_tokens)} in / {formatTokens(chat.usage.output_tokens)} out
            </span>
          )}
          <button className={`icon-btn ${panelOpen ? "on" : ""}`} onClick={togglePanel} title="Toggle side panel (Ctrl+\)">
            <PanelRight size={16} />
          </button>
        </div>
      </header>
      <Conversation />
      <Composer />
    </main>
  );
}

/** The chat's project, with a menu to move the chat to another one. */
function ProjectCrumb() {
  const sessionId = useStore((s) => s.sessionId);
  const projects = useStore((s) => s.projects);
  const groupId = useStore((s) => s.sessions.find((x) => x.id === s.sessionId)?.project_id ?? null);
  const running = useChat().running;
  const [menu, setMenu] = useState(false);
  const ref = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    if (!menu) return;
    const close = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && setMenu(false);
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [menu]);
  if (!sessionId || (!projects.length && !groupId)) return null;
  const group = projects.find((p) => p.id === groupId);
  const move = (id: string | null) => {
    setMenu(false);
    void useStore.getState().moveToProject(sessionId, id);
  };
  return (
    <span className="menu-anchor crumb-anchor" ref={ref}>
      <button className={`crumb crumb-btn ${group ? "" : "faint"}`} onClick={() => setMenu((m) => !m)} disabled={running} title={running ? "Wait for this chat to finish" : "Move this chat to a project"}>
        {group ? group.name : "No project"}
        <ChevronDown size={12} />
      </button>
      {menu && (
        <div className="menu down crumb-menu">
          <div className="menu-label">Move chat to</div>
          {projects.map((p) => (
            <button key={p.id} className="menu-item" onClick={() => move(p.id)}>
              <FolderOpen size={14} /> <span className="mi-text">{p.name}</span>
              {p.id === groupId && <Check size={14} />}
            </button>
          ))}
          <button className="menu-item subtle" onClick={() => move(null)}>
            No project {!group && <Check size={14} />}
          </button>
          <button
            className="menu-item subtle"
            onClick={() => {
              setMenu(false);
              useStore.getState().editProject(null);
            }}
          >
            New project…
          </button>
        </div>
      )}
      <span className="crumb-sep">/</span>
    </span>
  );
}

function OpenFolderButton() {
  const openProject = useStore((s) => s.openProject);
  return (
    <button
      className="btn"
      onClick={async () => {
        const dir = await open({ directory: true, multiple: false });
        if (typeof dir === "string") await openProject(dir);
      }}
    >
      <FolderOpen size={15} /> Open a folder
    </button>
  );
}

const CHAT_SUGGESTIONS = [
  "Explain how async/await works in Rust",
  "Write a regex that matches email addresses",
  "Help me plan the architecture for a new app",
  "Review this snippet for bugs",
];

const SUGGESTIONS = [
  "Give me a tour of this codebase",
  "Find and fix a bug in the tests",
  "Add a README section on how to run the project",
  "Review my uncommitted changes",
];

function Conversation() {
  const chat = useChat();
  const project = useStore((s) => s.project);
  const send = useStore((s) => s.send);
  const revert = useStore((s) => s.revert);
  const fullAuto = useStore((s) => s.settings?.permission_mode === "full_auto");
  const scroller = useRef<HTMLDivElement>(null);
  const [stick, setStick] = useState(true);
  const ts = useMemo(() => turns(chat.messages), [chat.messages]);
  const results = useMemo(() => {
    const m = resultIndex(chat.messages);
    for (const [id, r] of Object.entries(chat.liveResults)) if (!m.has(id)) m.set(id, r);
    return m;
  }, [chat.messages, chat.liveResults]);

  useEffect(() => {
    const el = scroller.current;
    if (el && stick) el.scrollTop = el.scrollHeight;
  }, [chat.messages, chat.draft, chat.approvals, stick]);

  const sessionId = useStore((s) => s.sessionId);
  useEffect(() => {
    if (!chat.running && sessionId) runStarts.delete(sessionId);
  }, [chat.running, sessionId]);

  const onScroll = () => {
    const el = scroller.current;
    if (el) setStick(el.scrollHeight - el.scrollTop - el.clientHeight < 80);
  };

  if (chat.messages.length === 0 && !chat.running) {
    return (
      <div className="empty-state">
        <div className="empty-mark" />
        <h2>What are we building?</h2>
        {project ? (
          <p>Describe a task. Casylum reads your code, plans, makes edits you can review, and runs commands.</p>
        ) : fullAuto ? (
          <p>Full auto is on and this chat has no folder, so Casylum can read, change and run anything on your PC without asking.</p>
        ) : (
          <p>Ask anything. To let Casylum read and edit your code, choose a folder for this chat, or switch to Full auto to give it your whole PC.</p>
        )}
        {!project && <OpenFolderButton />}
        <div className="suggestions">
          {(project ? SUGGESTIONS : CHAT_SUGGESTIONS).map((s) => (
            <button key={s} className="suggestion" onClick={() => send(s)}>
              {s}
            </button>
          ))}
        </div>
        {chat.error && <ErrorNote text={chat.error} />}
      </div>
    );
  }

  return (
    <div className="scroller" ref={scroller} onScroll={onScroll}>
      <div className="thread">
        {ts.map((t, ti) => {
          const latest = ti === ts.length - 1;
          // The turn being written right now gets no actions until it's done.
          const live = latest && chat.running;
          return (
            <div key={t.start} className={`turn ${latest ? "latest" : ""}`}>
              {chat.messages.slice(t.start, t.end).map((m, j) =>
                t.prompt && j === 0 ? (
                  <UserMessage key={t.start} message={m} turn={t} latest={latest} locked={chat.running} />
                ) : (
                  <MessageView key={t.start + j} message={m} results={results} />
                ),
              )}
              {t.prompt && hasReply(t) && !live && <ReplyActions turn={t} locked={chat.running} />}
            </div>
          );
        })}
        {chat.draft && (chat.draft.text || chat.draft.thinking || chat.draft.pendingTool) && (
          <div className="msg assistant">
            {chat.draft.thinking && <Thinking text={chat.draft.thinking} live />}
            {chat.draft.text && <Markdown text={chat.draft.text} />}
            {chat.draft.pendingTool && <div className="pending-tool">Preparing {chat.draft.pendingTool.replace("_", " ")}…</div>}
          </div>
        )}
        {chat.running && <WorkingLine waiting={Object.keys(chat.approvals).length > 0} />}
        {chat.error && <ErrorNote text={chat.error} />}
        {!chat.running && chat.lastRun && (
          <div className="run-summary">
            <span>
              Changed {chat.lastRun.files} file{chat.lastRun.files === 1 ? "" : "s"}
            </span>
            <button className="btn ghost sm" onClick={() => revert(chat.lastRun!.id)}>
              <RotateCcw size={13} /> Undo this run
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

// When each session's current run started, so the timer survives switching chats.
const runStarts = new Map<string, number>();

function elapsed(ms: number) {
  const s = Math.max(0, Math.floor(ms / 1000));
  return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, "0")}s`;
}

/** The live line at the bottom of a running chat: what the agent is doing and for how long. */
function WorkingLine({ waiting }: { waiting: boolean }) {
  const sessionId = useStore((s) => s.sessionId) ?? "";
  if (!runStarts.has(sessionId)) runStarts.set(sessionId, Date.now());
  const start = runStarts.get(sessionId)!;
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const t = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(t);
  }, []);
  return (
    <div className={`working ${waiting ? "waiting" : ""}`}>
      <span className="dots" aria-hidden="true">
        <span className="dot" />
        <span className="dot" />
        <span className="dot" />
      </span>
      <span role="status">{waiting ? "Waiting for your approval" : "Working"}</span>
      {!waiting && <span className="working-time" aria-hidden="true">for {elapsed(now - start)}</span>}
    </div>
  );
}

function ErrorNote({ text }: { text: string }) {
  const setModal = useStore((s) => s.setModal);
  const keyIssue = /api key|401|unauthor|invalid x-api-key|base url/i.test(text);
  return (
    <div className="error-note">
      <TriangleAlert size={15} />
      <span>{text}</span>
      {keyIssue && (
        <button className="btn sm" onClick={() => setModal("settings")}>
          Open settings
        </button>
      )}
    </div>
  );
}

function Thinking({ text, live }: { text: string; live?: boolean }) {
  const [open, setOpen] = useState(false);
  if (!text.trim()) return null;
  return (
    <div className={`thinking ${live ? "live" : ""}`}>
      <button className="thinking-head" onClick={() => setOpen((o) => !o)}>
        <Brain size={13} />
        <span>{live ? "Thinking…" : "Thought process"}</span>
        <ChevronDown size={13} className={open ? "rot" : ""} />
      </button>
      {(open || live) && <div className="thinking-body">{live ? text.slice(-600) : text}</div>}
    </div>
  );
}

function useCopy() {
  const [copied, setCopied] = useState(false);
  const copy = (text: string) => {
    void navigator.clipboard.writeText(text).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1400);
    });
  };
  return { copied, copy };
}

/** Delete asks once more on the same button, so a stray click can't lose a message. */
function DeleteButton({ label, onConfirm, disabled }: { label: string; onConfirm: () => void; disabled?: boolean }) {
  const [armed, setArmed] = useState(false);
  useEffect(() => {
    if (!armed) return;
    const t = window.setTimeout(() => setArmed(false), 3000);
    return () => window.clearTimeout(t);
  }, [armed]);
  return (
    <button
      className={`act danger ${armed ? "armed" : ""}`}
      disabled={disabled}
      onClick={() => (armed ? onConfirm() : setArmed(true))}
      onBlur={() => setArmed(false)}
      title={armed ? "Click again to delete" : label}
      aria-label={armed ? "Confirm delete" : label}
    >
      <Trash2 size={13} />
      {armed && <span>Delete?</span>}
    </button>
  );
}

function UserMessage({ message, turn, latest, locked }: { message: Message; turn: Turn; latest: boolean; locked: boolean }) {
  const deleteTurn = useStore((s) => s.deleteTurn);
  const retry = useStore((s) => s.retry);
  const [editing, setEditing] = useState(false);
  const { copied, copy } = useCopy();
  const { text, attachments } = promptParts(message);
  const texts = message.content.filter((b) => b.type === "text") as Array<{ type: "text"; text: string }>;

  if (editing) {
    return (
      <div className="msg user">
        <EditBox start={turn.start} text={text} attachments={attachments} latest={latest} onClose={() => setEditing(false)} />
      </div>
    );
  }

  return (
    <div className="msg user">
      <SentAttachments items={attachments} />
      {texts.map((b, i) => (
        <div className="bubble" key={i}>
          {b.text}
        </div>
      ))}
      <div className="msg-actions">
        <button className="act" onClick={() => copy(text)} title="Copy" aria-label="Copy message" disabled={!text}>
          {copied ? <Check size={13} /> : <Copy size={13} />}
        </button>
        <button className="act" onClick={() => setEditing(true)} disabled={locked} title={locked ? "Wait for Casylum to finish" : "Edit"} aria-label="Edit message">
          <Pencil size={13} />
        </button>
        {!hasReply(turn) && (
          <button className="act" onClick={() => void retry(turn.start)} disabled={locked} title="Send again" aria-label="Send again">
            <RotateCw size={13} />
          </button>
        )}
        <DeleteButton label="Delete message and its reply" disabled={locked} onConfirm={() => void deleteTurn(turn.start)} />
      </div>
    </div>
  );
}

function EditBox({ start, text, attachments, latest, onClose }: { start: number; text: string; attachments: Attachment[]; latest: boolean; onClose: () => void }) {
  const editAndSend = useStore((s) => s.editAndSend);
  const notify = useStore((s) => s.notify);
  const [value, setValue] = useState(text);
  const [atts, setAtts] = useState(attachments);
  const ta = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    const el = ta.current;
    if (!el) return;
    el.style.height = "0px";
    el.style.height = Math.min(el.scrollHeight, 320) + "px";
  }, [value]);

  useEffect(() => {
    const el = ta.current;
    el?.focus();
    el?.setSelectionRange(el.value.length, el.value.length);
  }, []);

  const empty = !value.trim() && !atts.length;
  const save = () => {
    if (empty) return;
    onClose();
    void editAndSend(start, value.trim(), atts);
  };

  return (
    <div className="edit-box">
      <AttachmentRow items={atts} onRemove={(i) => setAtts((a) => a.filter((_, j) => j !== i))} />
      <textarea
        ref={ta}
        value={value}
        onChange={(e) => setValue(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape") onClose();
          if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
            e.preventDefault();
            save();
          }
        }}
        rows={1}
      />
      <div className="edit-bar">
        <button
          className="icon-btn sm"
          title="Attach files"
          aria-label="Attach files"
          onClick={async () => {
            const picked = await open({ multiple: true, directory: false, title: "Attach files" });
            const paths = Array.isArray(picked) ? picked : typeof picked === "string" ? [picked] : [];
            const more = await readPaths(paths, notify);
            if (more.length) setAtts((a) => [...a, ...more]);
          }}
        >
          <Paperclip size={14} />
        </button>
        <span className="edit-hint">{latest ? "" : "Sending replaces everything after this message"}</span>
        <button className="btn ghost sm" onClick={onClose}>
          Cancel
        </button>
        <button className="btn primary sm" onClick={save} disabled={empty}>
          Send
        </button>
      </div>
    </div>
  );
}

function ReplyActions({ turn, locked }: { turn: Turn; locked: boolean }) {
  const chat = useChat();
  const retry = useStore((s) => s.retry);
  const deleteReply = useStore((s) => s.deleteReply);
  const { copied, copy } = useCopy();
  const text = replyText(chat.messages, turn);
  return (
    <div className="reply-actions">
      <button className="act" onClick={() => copy(text)} disabled={!text} title="Copy reply" aria-label="Copy reply">
        {copied ? <Check size={13} /> : <Copy size={13} />}
      </button>
      <button className="act" onClick={() => void retry(turn.start)} disabled={locked} title="Try again" aria-label="Try again">
        <RotateCw size={13} />
      </button>
      <DeleteButton label="Delete reply" disabled={locked} onConfirm={() => void deleteReply(turn.start)} />
    </div>
  );
}

function MessageView({ message, results }: { message: Message; results: ReturnType<typeof resultIndex> }) {
  const chat = useChat();
  // Tool results render inside their step cards.
  if (message.role === "user") return null;

  // Group consecutive tool calls into one "steps" stack.
  const groups: Array<{ kind: "steps"; tools: ToolUse[] } | { kind: "block"; block: Message["content"][number] }> = [];
  for (const b of message.content) {
    if (b.type === "tool_use") {
      const last = groups[groups.length - 1];
      if (last?.kind === "steps") last.tools.push(b);
      else groups.push({ kind: "steps", tools: [b] });
    } else groups.push({ kind: "block", block: b });
  }

  return (
    <div className="msg assistant">
      {groups.map((g, i) => (
        <Fragment key={i}>
          {g.kind === "steps" ? (
            <div className="steps">
              {g.tools.map((t) => (
                <StepCard key={t.id} tool={t} result={results.get(t.id)} active={chat.activeTool === t.id} approval={chat.approvals[t.id]} />
              ))}
            </div>
          ) : g.block.type === "text" ? (
            <Markdown text={g.block.text} />
          ) : g.block.type === "thinking" ? (
            <Thinking text={g.block.thinking} />
          ) : null}
        </Fragment>
      ))}
    </div>
  );
}

