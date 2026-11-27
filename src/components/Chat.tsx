import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import { Brain, ChevronDown, FolderOpen, PanelRight, RotateCcw, TriangleAlert } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import type { Message } from "../lib/api";
import { basename } from "../lib/api";
import { resultIndex, formatTokens, type ToolUse } from "../lib/steps";
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
      <header className="chat-head">
        <div className="chat-title">
          <span className="crumb">{project ? basename(project) : "No project"}</span>
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
      {project ? <Conversation /> : <NoProject />}
      {project && <Composer />}
    </main>
  );
}

function NoProject() {
  const openProject = useStore((s) => s.openProject);
  return (
    <div className="empty-state">
      <div className="empty-mark" />
      <h2>Open a project to start</h2>
      <p>Casylum works inside a folder on your computer. Pick one and ask for anything.</p>
      <button
        className="btn primary"
        onClick={async () => {
          const dir = await open({ directory: true, multiple: false });
          if (typeof dir === "string") await openProject(dir);
        }}
      >
        <FolderOpen size={15} /> Open folder
      </button>
    </div>
  );
}

const SUGGESTIONS = [
  "Give me a tour of this codebase",
  "Find and fix a bug in the tests",
  "Add a README section on how to run the project",
  "Review my uncommitted changes",
];

function Conversation() {
  const chat = useChat();
  const send = useStore((s) => s.send);
  const revert = useStore((s) => s.revert);
  const scroller = useRef<HTMLDivElement>(null);
  const [stick, setStick] = useState(true);
  const results = useMemo(() => {
    const m = resultIndex(chat.messages);
    for (const [id, r] of Object.entries(chat.liveResults)) if (!m.has(id)) m.set(id, r);
    return m;
  }, [chat.messages, chat.liveResults]);

  useEffect(() => {
    const el = scroller.current;
    if (el && stick) el.scrollTop = el.scrollHeight;
  }, [chat.messages, chat.draft, chat.approvals, stick]);

  const onScroll = () => {
    const el = scroller.current;
    if (el) setStick(el.scrollHeight - el.scrollTop - el.clientHeight < 80);
  };

  if (chat.messages.length === 0 && !chat.running) {
    return (
      <div className="empty-state">
        <div className="empty-mark" />
        <h2>What are we building?</h2>
        <p>Describe a task. Casylum reads your code, plans, makes edits you can review, and runs commands.</p>
        <div className="suggestions">
          {SUGGESTIONS.map((s) => (
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
        {chat.messages.map((m, i) => (
          <MessageView key={i} message={m} results={results} />
        ))}
        {chat.draft && (chat.draft.text || chat.draft.thinking || chat.draft.pendingTool) && (
          <div className="msg assistant">
            {chat.draft.thinking && <Thinking text={chat.draft.thinking} live />}
            {chat.draft.text && <Markdown text={chat.draft.text} />}
            {chat.draft.pendingTool && <div className="pending-tool">Preparing {chat.draft.pendingTool.replace("_", " ")}…</div>}
          </div>
        )}
        {chat.running && !chat.draft?.text && !chat.draft?.pendingTool && !Object.keys(chat.approvals).length && (
          <div className="working">
            <span className="dot" />
            <span className="dot" />
            <span className="dot" />
          </div>
        )}
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

function MessageView({ message, results }: { message: Message; results: ReturnType<typeof resultIndex> }) {
  const chat = useChat();
  if (message.role === "user") {
    const texts = message.content.filter((b) => b.type === "text");
    if (!texts.length) return null; // tool results render inside their step cards
    return (
      <div className="msg user">
        {texts.map((b, i) => (
          <div className="bubble" key={i}>
            {(b as any).text}
          </div>
        ))}
      </div>
    );
  }

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

