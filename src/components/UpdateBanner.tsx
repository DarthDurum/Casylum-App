import { useEffect } from "react";
import { Download, X } from "lucide-react";
import { startAutoUpdates, useUpdates } from "../lib/updates";
import { useStore } from "../store";

// Checks GitHub Releases at startup and every few hours. With auto-update on,
// the new version downloads silently and this banner only says it's ready;
// with it off, the banner offers the update.
export function UpdateBanner() {
  const { status, update, progress, dismissed, error, install, dismiss } = useUpdates();
  const auto = useStore((s) => s.settings?.auto_update ?? true);

  useEffect(() => {
    // The browser preview (pnpm dev) has no real updater.
    if (import.meta.env.PROD) startAutoUpdates(() => useStore.getState().settings?.auto_update ?? true);
  }, []);

  if (!update || dismissed) return null;
  // Auto mode stays silent until the download is done (or fails).
  const visible = auto ? status === "ready" || status === "installing" || status === "error" : ["available", "downloading", "installing", "ready", "error"].includes(status);
  if (!visible) return null;

  const busy = status === "downloading" || status === "installing";
  const note =
    status === "downloading"
      ? `Downloading… ${Math.round(progress * 100)}%`
      : status === "installing"
        ? "Installing…"
        : status === "error"
          ? `Update failed: ${error}`
          : status === "ready"
            ? "It installs when you close Casylum, or restart now."
            : "Casylum restarts to finish updating.";

  return (
    <div className="update-banner" role="status">
      <Download size={15} />
      <div className="update-text">
        <strong>{status === "ready" ? `Casylum ${update.version} is ready` : `Casylum ${update.version} is available`}</strong>
        <span className="muted small">{note}</span>
      </div>
      <button className="btn primary sm" onClick={() => void install()} disabled={busy}>
        {busy ? "Updating…" : status === "error" ? "Retry" : status === "ready" ? "Restart now" : "Update"}
      </button>
      {!busy && (
        <button className="icon-btn" title="Later" onClick={dismiss}>
          <X size={14} />
        </button>
      )}
    </div>
  );
}
