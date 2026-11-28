import { useState } from "react";
import { Check, ExternalLink, KeyRound, Lock, Trash2 } from "lucide-react";
import { api, type Provider } from "../lib/api";
import { useStore } from "../store";
import { Modal } from "./Modal";
import { useUpdates } from "../lib/updates";
import { SkillsPane } from "./Skills";
import { McpPane } from "./Mcp";
import type { SettingsTab } from "../store";

const TABS: Array<[SettingsTab, string]> = [
  ["providers", "Providers & keys"],
  ["mcp", "MCP servers"],
  ["skills", "Skills"],
  ["general", "General"],
];

export function SettingsModal() {
  const setModal = useStore((s) => s.setModal);
  const providers = useStore((s) => s.providers);
  const settings = useStore((s) => s.settings)!;
  const update = useStore((s) => s.updateSettings);
  const tab = useStore((s) => s.settingsTab);
  const setTab = (t: SettingsTab) => useStore.setState({ settingsTab: t });
  // Skill and JSON editors fill the page instead of scrolling with it.
  const [flush, setFlush] = useState(false);
  const [filter, setFilter] = useState("");
  const f = filter.trim().toLowerCase();
  // Connected providers first, then the rest in catalog order.
  const shown = providers
    .filter((p) => !f || p.name.toLowerCase().includes(f) || p.id.includes(f))
    .sort((a, b) => Number(b.has_key || b.id === settings.provider) - Number(a.has_key || a.id === settings.provider));

  return (
    <Modal title="Settings" onClose={() => setModal(null)} wide>
      <div className="settings">
        <nav className="settings-nav">
          {TABS.map(([t, label]) => (
            <button
              key={t}
              className={tab === t ? "on" : ""}
              onClick={() => {
                setFlush(false);
                setTab(t);
              }}
            >
              {label}
            </button>
          ))}
        </nav>
        <div className={`settings-main ${flush && (tab === "skills" || tab === "mcp") ? "flush" : ""}`}>
          {tab === "skills" && <SkillsPane onEditing={setFlush} />}
          {tab === "mcp" && <McpPane onEditing={setFlush} />}
          {tab === "providers" && (
            <>
              <p className="muted lock-note">
                <Lock size={13} /> Keys are stored in your operating system's keychain and never leave this computer except to call the provider.
              </p>
              <input className="provider-filter" placeholder={`Search ${providers.length} providers…`} value={filter} onChange={(e) => setFilter(e.target.value)} />
              {shown.map((p) => (
                <ProviderCard key={p.id} p={p} />
              ))}
              {shown.length === 0 && <p className="muted small">No provider matches “{filter}”. Use Custom (OpenAI-compatible) for any other API.</p>}
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
  const urlField = p.url_setting;
  const isUrlProvider = !!urlField;
  const [url, setUrl] = useState(urlField ? settings[urlField] : "");
  const active = settings.provider === p.id;

  const save = async () => {
    setBusy(true);
    try {
      if (key.trim()) await api.setApiKey(p.id, key.trim());
      if (urlField) await update({ [urlField]: url.trim() });
      setKey("");
      await refresh();
      const list = await useStore.getState().loadModels(p.id, true);
      if (!settings.provider || !useStore.getState().providers.find((x) => x.id === settings.provider)?.has_key) {
        const model = list?.models[0]?.id ?? p.models[0];
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
            {p.tools === false && <span title="This provider's API can't call tools, so it can chat but not edit files or run commands."> · Chat only</span>}
          </div>
        </div>
        {p.key_url && (
          <a className="link-btn" href={p.key_url} target="_blank" rel="noreferrer">
            {p.needs_key ? "Get a key" : "Docs"} <ExternalLink size={12} />
          </a>
        )}
      </div>
      <div className="provider-form">
        {isUrlProvider && <input placeholder={`Base URL, e.g. ${p.base_url || "https://api.example.com/v1"}`} value={url} onChange={(e) => setUrl(e.target.value)} />}
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
                void useStore.getState().loadModels(p.id, true);
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
  const settings = useStore((s) => s.settings)!;
  const updateSettings = useStore((s) => s.updateSettings);
  const auto = settings.auto_update;
  const label =
    status === "checking"
      ? "Checking…"
      : status === "downloading"
        ? "Downloading the update…"
        : status === "installing"
          ? "Installing…"
          : status === "ready" && update
            ? `Version ${update.version} is downloaded. It installs when you close Casylum.`
            : update && status !== "none"
              ? `Version ${update.version} is available.`
              : status === "none"
                ? "You're on the latest version."
                : status === "error"
                  ? `Couldn't update: ${error}`
                  : "Updates come from GitHub Releases.";
  return (
    <>
      <label>
        <span>Automatic updates</span>
        <div className="seg">
          {(
            [
              [true, "On"],
              [false, "Off"],
            ] as const
          ).map(([v, l]) => (
            <button key={l} className={auto === v ? "on" : ""} onClick={() => updateSettings({ auto_update: v })}>
              {l}
            </button>
          ))}
        </div>
      </label>
      <div className="form-field">
        <span>Updates{current && <span className="muted"> · version {current}</span>}</span>
        <div className="update-row">
          <span className="muted small">{label}</span>
          {update && status !== "none" && status !== "checking" ? (
            <button className="btn primary sm" onClick={() => void install()} disabled={status === "downloading" || status === "installing"}>
              {status === "ready" ? "Restart now" : "Update and restart"}
            </button>
          ) : (
            <button className="btn sm" onClick={() => void check({ manual: true, auto })} disabled={status === "checking"}>
              Check for updates
            </button>
          )}
        </div>
      </div>
    </>
  );
}
