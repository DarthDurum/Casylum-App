import { useState } from "react";
import { Check, ExternalLink, KeyRound, Lock, Trash2 } from "lucide-react";
import { api, type Provider } from "../lib/api";
import { useStore } from "../store";
import { Modal } from "./Modal";
import { useUpdates } from "../lib/updates";

export function SettingsModal() {
  const setModal = useStore((s) => s.setModal);
  const providers = useStore((s) => s.providers);
  const settings = useStore((s) => s.settings)!;
  const update = useStore((s) => s.updateSettings);
  const [tab, setTab] = useState<"providers" | "general">("providers");

  return (
    <Modal title="Settings" onClose={() => setModal(null)} wide>
      <div className="settings">
        <nav className="settings-nav">
          <button className={tab === "providers" ? "on" : ""} onClick={() => setTab("providers")}>
            Providers & keys
          </button>
          <button className={tab === "general" ? "on" : ""} onClick={() => setTab("general")}>
            General
          </button>
        </nav>
        <div className="settings-main">
          {tab === "providers" && (
            <>
              <p className="muted lock-note">
                <Lock size={13} /> Keys are stored in your operating system's keychain and never leave this computer except to call the provider.
              </p>
              {providers.map((p) => (
                <ProviderCard key={p.id} p={p} />
              ))}
            </>
          )}
          {tab === "general" && (
            <div className="form">
              <label>
                <span>Theme</span>
                <div className="seg">
                  {(["dark", "light"] as const).map((t) => (
                    <button key={t} className={settings.theme === t ? "on" : ""} onClick={() => update({ theme: t })}>
                      {t === "dark" ? "Dark" : "Light"}
                    </button>
                  ))}
                </div>
              </label>
              <label>
                <span>When the agent wants to change things</span>
                <div className="seg">
                  {(
                    [
                      ["ask", "Ask first"],
                      ["auto_edit", "Auto-edit"],
                      ["full_auto", "Full auto"],
                    ] as const
                  ).map(([v, l]) => (
                    <button key={v} className={settings.permission_mode === v ? "on" : ""} onClick={() => update({ permission_mode: v })}>
                      {l}
                    </button>
                  ))}
                </div>
              </label>
              <UpdatesRow />
            </div>
          )}
        </div>
      </div>
    </Modal>
  );
}

function ProviderCard({ p }: { p: Provider }) {
  const refresh = useStore((s) => s.refreshProviders);
  const settings = useStore((s) => s.settings)!;
  const update = useStore((s) => s.updateSettings);
  const notify = useStore((s) => s.notify);
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const isUrlProvider = p.id === "ollama" || p.id === "custom";
  const urlField = p.id === "ollama" ? "ollama_base_url" : "custom_base_url";
  const [url, setUrl] = useState(isUrlProvider ? (settings as any)[urlField] : "");
  const active = settings.provider === p.id;

  const save = async () => {
    setBusy(true);
    try {
      if (key.trim()) await api.setApiKey(p.id, key.trim());
      if (isUrlProvider) await update({ [urlField]: url.trim() } as any);
      setKey("");
      await refresh();
      if (!settings.provider || !useStore.getState().providers.find((x) => x.id === settings.provider)?.has_key) {
        const model = p.models[0];
        if (model) await update({ provider: p.id, model });
      }
      notify(`${p.name} saved`);
    } catch (e) {
      notify(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className={`provider ${p.has_key ? "ready" : ""}`}>
      <div className="provider-head">
        <div>
          <div className="provider-name">
            {p.name}
            {active && <span className="tag">In use</span>}
          </div>
          <div className="muted small">
            {p.has_key ? (
              <>
                <Check size={12} /> Key saved <span className="mono">{p.key_hint}</span>
              </>
            ) : p.needs_key ? (
              "No key yet"
            ) : (
              "No key needed"
            )}
          </div>
        </div>
        {p.key_url && (
          <a className="link-btn" href={p.key_url} target="_blank" rel="noreferrer">
            {p.needs_key ? "Get a key" : "Docs"} <ExternalLink size={12} />
          </a>
        )}
      </div>
      <div className="provider-form">
        {isUrlProvider && <input placeholder="Base URL, e.g. http://localhost:11434/v1" value={url} onChange={(e) => setUrl(e.target.value)} />}
        <div className="key-row">
          <KeyRound size={14} />
          <input
            type="password"
            placeholder={p.has_key ? "Replace key…" : p.needs_key ? "Paste API key" : "API key (optional)"}
            value={key}
            onChange={(e) => setKey(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && save()}
          />
          <button className="btn primary sm" onClick={save} disabled={busy || (!key.trim() && !isUrlProvider)}>
            Save
          </button>
          {p.has_key && (
            <button
              className="icon-btn"
              title="Remove key"
              onClick={async () => {
                await api.deleteApiKey(p.id);
                await refresh();
              }}
            >
              <Trash2 size={14} />
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

function UpdatesRow() {
  const { current, status, update, error, check, install } = useUpdates();
  const label =
    status === "checking"
      ? "Checking…"
      : status === "downloading"
        ? "Downloading the update…"
        : update && status !== "none"
          ? `Version ${update.version} is available.`
          : status === "none"
            ? "You're on the latest version."
            : status === "error"
              ? `Couldn't check for updates: ${error}`
              : "Updates come from GitHub Releases.";
  return (
    <div className="form-field">
      <span>Updates{current && <span className="muted"> · version {current}</span>}</span>
      <div className="update-row">
        <span className="muted small">{label}</span>
        {update && status !== "none" ? (
          <button className="btn primary sm" onClick={() => void install()} disabled={status === "downloading"}>
            Update and restart
          </button>
        ) : (
          <button className="btn sm" onClick={() => void check(true)} disabled={status === "checking"}>
            Check for updates
          </button>
        )}
      </div>
    </div>
  );
}
