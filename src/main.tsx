import React from "react";
import ReactDOM from "react-dom/client";
import "@fontsource-variable/instrument-sans/standard.css";
import "@fontsource-variable/geist-mono";
import "./styles.css";
import App from "./App";

async function boot() {
  // In a plain browser (pnpm dev), fake the Rust core for UI work.
  if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
    (await import("./dev/mock")).installMock();
  }
  ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
  );
}

void boot();
