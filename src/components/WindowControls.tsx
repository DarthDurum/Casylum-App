import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

// The window has no OS title bar (decorations: false), so the app draws its
// own minimize, maximize/restore and close buttons. Headers carry
// data-tauri-drag-region="deep" so the window can still be dragged.

function win() {
  try {
    return getCurrentWindow();
  } catch {
    return null; // plain browser preview
  }
}

export function WindowControls() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const w = win();
    if (!w) return;
    const sync = () => void w.isMaximized().then(setMaximized).catch(() => {});
    sync();
    const un = w.onResized(sync).catch(() => undefined);
    return () => void un.then((f) => f?.());
  }, []);

  return (
    <div className="win-controls" role="group" aria-label="Window">
      <button className="win-btn" onClick={() => void win()?.minimize()} title="Minimize" aria-label="Minimize">
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <path d="M1 5.5h8" />
        </svg>
      </button>
      <button
        className="win-btn"
        onClick={() => void win()?.toggleMaximize()}
        title={maximized ? "Restore" : "Maximize"}
        aria-label={maximized ? "Restore" : "Maximize"}
      >
        {maximized ? (
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
            <rect x="1" y="3" width="6" height="6" rx="1.2" />
            <path d="M3 3V2.2C3 1.5 3.5 1 4.2 1h3.6C8.5 1 9 1.5 9 2.2v3.6C9 6.5 8.5 7 7.8 7H7" />
          </svg>
        ) : (
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
            <rect x="1" y="1" width="8" height="8" rx="1.6" />
          </svg>
        )}
      </button>
      <button className="win-btn close" onClick={() => void win()?.close()} title="Close" aria-label="Close">
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <path d="M1.5 1.5l7 7M8.5 1.5l-7 7" />
        </svg>
      </button>
    </div>
  );
}
