import { useState } from "react";
import { ArrowRight, ExternalLink, FolderOpen, Lock } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "../lib/api";
import { useStore } from "../store";

/** First run: connect one provider, then optionally pick a folder. */
export function Onboarding({ onDone }: { onDone: () => void }) {
  const providers = useStore((s) => s.providers);
  const refresh = useStore((s) => s.refreshProviders);
  const update = useStore((s) => s.updateSettings);
  const openProject = useStore((s) => s.openProject);
  const project = useStore((s) => s.project);
  const [pid, setPid] = useState("anthropic");
  const [key, setKey] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const settings = useStore((s) => s.settings);
  const connected = providers.some((p) => p.has_key) || ["ollama", "custom"].includes(settings?.provider ?? "");
  const [step, setStep] = useState<1 | 2>(connected ? 2 : 1);
  const p = providers.find((x) => x.id === pid);

  const saveKey = async () => {
    setErr(null);
    try {
      if (p?.needs_key) {
        if (!key.trim()) return setErr("Paste a key to continue.");
        await api.setApiKey(pid, key.trim());
      }
      await refresh();
      const list = await useStore.getState().loadModels(pid, true);
      await update({ provider: pid, model: list?.models[0]?.id ?? p?.models[0] ?? "" });
      setStep(2);
    } catch (e) {
      setErr(String(e));
    }
  };

  return (
    <div className="onboard">
      <div className="onboard-card">
        <div className="brand big">
          <div className="brand-mark" />
          <span>Casylum</span>
        </div>
        <p className="onboard-sub">A calm, fast place to build with AI. Bring your own keys.</p>

        <div className="steps-dots">
          <span className={step >= 1 ? "on" : ""} />
          <span className={step >= 2 ? "on" : ""} />
        </div>

        {step === 1 && (
          <>
            <h3>Connect a model provider</h3>
            <div className="provider-grid">
              {providers
                .filter((x) => x.id !== "custom")
                .map((x) => (
                  <button key={x.id} className={`provider-pick ${pid === x.id ? "on" : ""}`} onClick={() => setPid(x.id)}>
                    {x.name}
                  </button>
                ))}
            </div>
            {p?.needs_key ? (
              <div className="key-row big">
                <input type="password" placeholder={`${p.name} API key`} value={key} autoFocus onChange={(e) => setKey(e.target.value)} onKeyDown={(e) => e.key === "Enter" && saveKey()} />
              </div>
            ) : (
              <p className="muted small">Make sure Ollama is running locally. You can change its address in Settings.</p>
            )}
            <div className="onboard-row">
              {p?.key_url && (
                <a className="link-btn" href={p.key_url} target="_blank" rel="noreferrer">
                  Where do I get one? <ExternalLink size={12} />
                </a>
              )}
              <div className="spacer" />
              <button className="btn primary" onClick={saveKey}>
                Continue <ArrowRight size={14} />
              </button>
            </div>
            <p className="muted small lock-note">
              <Lock size={12} /> Stored in your OS keychain. Never synced, never logged.
            </p>
          </>
        )}

        {step === 2 && (
          <>
            <h3>Open a project</h3>
            <p className="muted">Pick a folder so Casylum can read and edit your code, or skip and just chat. You can open a folder any time.</p>
            <div className="onboard-row">
              <button className="btn ghost" onClick={() => setStep(1)}>
                Back
              </button>
              <div className="spacer" />
              {!project && (
                <button className="btn" onClick={onDone}>
                  Skip, just chat
                </button>
              )}
              {project && (
                <button className="btn" onClick={onDone}>
                  Keep {project.split(/[\\/]/).pop()}
                </button>
              )}
              <button
                className="btn primary"
                onClick={async () => {
                  const d = await open({ directory: true });
                  if (typeof d === "string") {
                    await openProject(d);
                    onDone();
                  }
                }}
              >
                <FolderOpen size={14} /> Choose folder
              </button>
            </div>
          </>
        )}
        {err && <div className="error-note">{err}</div>}
      </div>
    </div>
  );
}
