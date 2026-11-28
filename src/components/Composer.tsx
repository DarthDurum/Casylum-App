import { useEffect, useRef, useState } from "react";
import { ArrowUp, Check, ChevronDown, Paperclip, RefreshCw, Shield, ShieldCheck, ShieldOff, Square } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import type { Attachment, PermissionMode } from "../lib/api";
import { AttachmentRow, readPastedImages, readPaths } from "./Attachments";
import { modelLabel, resolveTypedModel, type ModelInfo } from "../lib/models";
import { useChat, useStore } from "../store";

const MODES: Record<PermissionMode, { label: string; hint: string; icon: any }> = {
  ask: { label: "Ask first", hint: "Approve every edit and command", icon: Shield },
  auto_edit: { label: "Auto-edit", hint: "Edits apply, commands ask", icon: ShieldCheck },
  full_auto: { label: "Full auto", hint: "Never asks. Use with care", icon: ShieldOff },
};

export function Composer() {
  const chat = useChat();
  const send = useStore((s) => s.send);
  const stop = useStore((s) => s.stop);
  const settings = useStore((s) => s.settings);
  const update = useStore((s) => s.updateSettings);
  const sessionId = useStore((s) => s.sessionId);
  const providerModels = useStore((s) => (s.settings ? s.models[s.settings.provider]?.models : undefined));
  const providerName = useStore((s) => s.providers.find((p) => p.id === s.settings?.provider)?.name);
  const notify = useStore((s) => s.notify);
  const [text, setText] = useState("");
  const [atts, setAtts] = useState<Attachment[]>([]);
  const [dropping, setDropping] = useState(false);
  const [menu, setMenu] = useState<null | "model" | "mode">(null);
  const ta = useRef<HTMLTextAreaElement>(null);

  const add = (more: Attachment[]) => {
    if (more.length) setAtts((a) => [...a, ...more]);
    ta.current?.focus();
  };

  const pickFiles = async () => {
    const picked = await open({ multiple: true, directory: false, title: "Attach files" });
    const paths = Array.isArray(picked) ? picked : typeof picked === "string" ? [picked] : [];
    add(await readPaths(paths, notify));
  };

  // Files dragged onto the window arrive as paths from Tauri, not as DOM drops.
  useEffect(() => {
    let un: (() => void) | undefined;
    let gone = false;
    try {
      void getCurrentWebview()
        .onDragDropEvent((e) => {
          const p = e.payload;
          if (p.type === "enter" || p.type === "over") setDropping(true);
          else if (p.type === "leave") setDropping(false);
          else if (p.type === "drop") {
            setDropping(false);
            void readPaths(p.paths, useStore.getState().notify).then(add);
          }
        })
        .then((f) => (gone ? f() : (un = f)))
        .catch(() => {});
    } catch {
      // plain browser preview
    }
    return () => {
      gone = true;
      un?.();
    };
  }, []);

  useEffect(() => {
    ta.current?.focus();
  }, [sessionId]);

  useEffect(() => {
    const el = ta.current;
    if (!el) return;
    el.style.height = "0px";
    el.style.height = Math.min(el.scrollHeight, 260) + "px";
  }, [text]);

  useEffect(() => {
    const close = () => setMenu(null);
    if (menu) window.addEventListener("click", close);
    return () => window.removeEventListener("click", close);
  }, [menu]);

  const submit = () => {
    const t = text.trim();
    if ((!t && !atts.length) || chat.running) return;
    setText("");
    setAtts([]);
    void send(t, atts);
  };

  if (!settings) return null;
  const mode = MODES[settings.permission_mode];
  const ModeIcon = mode.icon;

  return (
    <div className="composer-wrap">
      <div className={`composer ${chat.running ? "busy" : ""} ${dropping ? "dropping" : ""}`}>
        {dropping && <div className="drop-hint">Drop files to attach</div>}
        <AttachmentRow items={atts} onRemove={(i) => setAtts((a) => a.filter((_, j) => j !== i))} />
        <textarea
          ref={ta}
          value={text}
          placeholder={chat.running ? "Casylum is working… you can type your next message" : "Ask Casylum to build, fix or explain…"}
          onChange={(e) => setText(e.target.value)}
          onPaste={(e) => {
            const files = Array.from(e.clipboardData.files);
            if (!files.some((f) => f.type.startsWith("image/"))) return;
            e.preventDefault();
            void readPastedImages(files, notify).then(add);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault();
              submit();
            }
          }}
          rows={1}
        />
        <div className="composer-bar">
          <div className="pill-row">
            <button className="pill icon-pill" onClick={() => void pickFiles()} title="Attach files or images" aria-label="Attach files">
              <Paperclip size={14} />
            </button>
            <div className="menu-anchor" onClick={(e) => e.stopPropagation()}>
              <button className={`pill mode-${settings.permission_mode}`} onClick={() => setMenu(menu === "mode" ? null : "mode")}>
                <ModeIcon size={12} />
                {mode.label}
                <ChevronDown size={12} />
              </button>
              {menu === "mode" && (
                <div className="menu up">
                  {(Object.keys(MODES) as PermissionMode[]).map((m) => {
                    const I = MODES[m].icon;
                    return (
                      <button
                        key={m}
                        className="menu-item"
                        onClick={() => {
                          void update({ permission_mode: m });
                          setMenu(null);
                        }}
                      >
                        <I size={14} />
                        <span className="mi-main">
                          {MODES[m].label}
                          <small>{MODES[m].hint}</small>
                        </span>
                        {settings.permission_mode === m && <Check size={14} />}
                      </button>
                    );
                  })}
                </div>
              )}
            </div>
          </div>
          <div className="composer-end">
            <div className="menu-anchor" onClick={(e) => e.stopPropagation()}>
              <button className="pill" onClick={() => setMenu(menu === "model" ? null : "model")} title={`Choose model (${providerName ?? settings.provider}: ${settings.model})`}>
                {modelLabel(settings.model, providerModels)}
                {providerName && <span className="pill-sub">{providerName}</span>}
                <ChevronDown size={12} />
              </button>
              {menu === "model" && <ModelMenu close={() => setMenu(null)} />}
            </div>
            {chat.running ? (
            <button className="send stop" onClick={() => stop()} title="Stop">
              <Square size={13} fill="currentColor" />
            </button>
          ) : (
            <button className="send" onClick={submit} disabled={!text.trim() && !atts.length} title="Send (Enter)">
              <ArrowUp size={16} />
            </button>
          )}
          </div>
        </div>
      </div>
    </div>
  );
}

function ModelMenu({ close }: { close: () => void }) {
  const providers = useStore((s) => s.providers);
  const models = useStore((s) => s.models);
  const loading = useStore((s) => s.modelsLoading);
  const loadModels = useStore((s) => s.loadModels);
  const settings = useStore((s) => s.settings)!;
  const update = useStore((s) => s.updateSettings);
  const setModal = useStore((s) => s.setModal);
  const [query, setQuery] = useState("");
  const usable = providers.filter((p) => p.has_key || !p.needs_key);
  const q = query.trim().toLowerCase();

  useEffect(() => {
    for (const p of usable) if (!models[p.id] && !loading[p.id]) void loadModels(p.id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const pick = (provider: string, model: string) => {
    void update({ provider, model });
    close();
  };

  return (
    <div className="menu up wide end model-menu">
      <form
        className="menu-search"
        onSubmit={(e) => {
          e.preventDefault();
          if (!query.trim()) return;
          // Send the model to the provider that lists it, not whichever one is active.
          const lists = usable.map((p) => ({ provider: p.id, models: models[p.id]?.models ?? p.models.map((id) => ({ id })) }));
          const r = resolveTypedModel(query, lists, settings.provider);
          pick(r.provider, r.model);
        }}
      >
        <input autoFocus placeholder="Search models, or type a model id…" value={query} onChange={(e) => setQuery(e.target.value)} />
      </form>
      {usable.length === 0 && <div className="menu-note">Add an API key to choose a model.</div>}
      {usable.map((p) => {
        const list = models[p.id];
        const all: ModelInfo[] = list?.models ?? p.models.map((id) => ({ id }));
        const shown = q ? all.filter((m) => m.id.toLowerCase().includes(q) || modelLabel(m.id, all).toLowerCase().includes(q)) : all;
        if (q && shown.length === 0) return null;
        // A local endpoint that isn't set up yet has nothing to offer.
        if (list && all.length === 0) return null;
        return (
          <div key={p.id} className="menu-group">
            <div className="menu-label">
              {p.name}
              <span className="spacer" />
              <button
                className="icon-btn xs"
                title={list?.error ? `Couldn't refresh: ${list.error}` : "Refresh model list"}
                onClick={() => void loadModels(p.id, true)}
              >
                <RefreshCw size={11} className={loading[p.id] ? "spin" : ""} />
              </button>
            </div>
            {list?.error && list.source !== "live" && (
              <div className="menu-hint" title={list.error}>
                {list.source === "cache" ? "Showing the last list we fetched." : "Couldn't reach the provider. Showing defaults."}
              </div>
            )}
            {!list && loading[p.id] && <div className="menu-hint">Loading models…</div>}
            {shown.map((m) => (
              <button key={m.id} className="menu-item" onClick={() => pick(p.id, m.id)} title={m.id}>
                <span className="mi-main">
                  {modelLabel(m.id, all)}
                  <small className="mono">{m.id}</small>
                </span>
                {settings.provider === p.id && settings.model === m.id && <Check size={14} />}
              </button>
            ))}
          </div>
        );
      })}
      <button
        className="menu-item subtle"
        onClick={() => {
          setModal("settings");
          close();
        }}
      >
        Manage providers and keys…
      </button>
    </div>
  );
}
