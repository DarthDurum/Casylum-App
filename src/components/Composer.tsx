import { useEffect, useRef, useState } from "react";
import { ArrowUp, Check, ChevronDown, Shield, ShieldCheck, ShieldOff, Square } from "lucide-react";
import type { PermissionMode } from "../lib/api";
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
  const [text, setText] = useState("");
  const [menu, setMenu] = useState<null | "model" | "mode">(null);
  const ta = useRef<HTMLTextAreaElement>(null);

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
    if (!t || chat.running) return;
    setText("");
    void send(t);
  };

  if (!settings) return null;
  const mode = MODES[settings.permission_mode];
  const ModeIcon = mode.icon;

  return (
    <div className="composer-wrap">
      <div className={`composer ${chat.running ? "busy" : ""}`}>
        <textarea
          ref={ta}
          value={text}
          placeholder={chat.running ? "Casylum is working… you can type your next message" : "Ask Casylum to build, fix or explain…"}
          onChange={(e) => setText(e.target.value)}
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
            <div className="menu-anchor" onClick={(e) => e.stopPropagation()}>
              <button className="pill" onClick={() => setMenu(menu === "model" ? null : "model")}>
                <span className="pill-dot" />
                {settings.model}
                <ChevronDown size={12} />
              </button>
              {menu === "model" && <ModelMenu close={() => setMenu(null)} />}
            </div>
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
          {chat.running ? (
            <button className="send stop" onClick={() => stop()} title="Stop">
              <Square size={13} fill="currentColor" />
            </button>
          ) : (
            <button className="send" onClick={submit} disabled={!text.trim()} title="Send (Enter)">
              <ArrowUp size={16} />
            </button>
          )}
        </div>
      </div>
      <div className="composer-hint">Enter to send · Shift+Enter for a new line · Ctrl+K for commands</div>
    </div>
  );
}

function ModelMenu({ close }: { close: () => void }) {
  const providers = useStore((s) => s.providers);
  const settings = useStore((s) => s.settings)!;
  const update = useStore((s) => s.updateSettings);
  const setModal = useStore((s) => s.setModal);
  const [custom, setCustom] = useState("");
  const usable = providers.filter((p) => p.has_key || !p.needs_key);

  const pick = (provider: string, model: string) => {
    void update({ provider, model });
    close();
  };

  return (
    <div className="menu up wide">
      {usable.length === 0 && <div className="menu-note">Add an API key to choose a model.</div>}
      {usable.map((p) => (
        <div key={p.id} className="menu-group">
          <div className="menu-label">{p.name}</div>
          {p.models.map((m) => (
            <button key={m} className="menu-item" onClick={() => pick(p.id, m)}>
              <span className="mi-main mono">{m}</span>
              {settings.provider === p.id && settings.model === m && <Check size={14} />}
            </button>
          ))}
        </div>
      ))}
      <form
        className="menu-custom"
        onSubmit={(e) => {
          e.preventDefault();
          if (custom.trim()) pick(settings.provider, custom.trim());
        }}
      >
        <input placeholder={`Other ${settings.provider} model id…`} value={custom} onChange={(e) => setCustom(e.target.value)} />
      </form>
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
