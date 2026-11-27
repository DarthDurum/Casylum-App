import { useEffect } from "react";
import { Download, X } from "lucide-react";
import { useUpdates } from "../lib/updates";

// Checks GitHub Releases once at startup and offers the new version.
export function UpdateBanner() {
  const { status, update, progress, dismissed, error, install, dismiss } = useUpdates();

  useEffect(() => {
    // The browser preview (pnpm dev) has no real updater.
    if (import.meta.env.PROD) void useUpdates.getState().check();
  }, []);

  const busy = status === "downloading";
  if (!update || dismissed || !(status === "available" || busy || (status === "error" && update))) return null;

  return (
    <div className="update-banner" role="status">
      <Download size={15} />
      <div className="update-text">
        <strong>Casylum {update.version} is available</strong>
        <span className="muted small">
          {busy ? `Downloading… ${Math.round(progress * 100)}%` : status === "error" ? `Update failed: ${error}` : "Casylum restarts to finish updating."}
        </span>
      </div>
      <button className="btn primary sm" onClick={() => void install()} disabled={busy}>
        {busy ? "Updating…" : status === "error" ? "Retry" : "Update"}
      </button>
      {!busy && (
        <button className="icon-btn" title="Later" onClick={dismiss}>
          <X size={14} />
        </button>
      )}
    </div>
  );
}
