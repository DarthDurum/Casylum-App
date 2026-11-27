import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { AgentEvent } from "./lib/api";
import { useStore } from "./store";
import { Rail } from "./components/Rail";
import { Chat } from "./components/Chat";
import { ContextPanel } from "./components/ContextPanel";
import { SettingsModal } from "./components/Settings";
import { SkillsModal } from "./components/Skills";
import { Palette } from "./components/Palette";
import { Onboarding } from "./components/Onboarding";
import { UpdateBanner } from "./components/UpdateBanner";

export default function App() {
  const ready = useStore((s) => s.ready);
  const modal = useStore((s) => s.modal);
  const panelOpen = useStore((s) => s.panelOpen);
  const toast = useStore((s) => s.toast);
  const providers = useStore((s) => s.providers);
  const project = useStore((s) => s.project);
  const settings = useStore((s) => s.settings);
  const [onboarded, setOnboarded] = useState(() => localStorage.getItem("onboarded") === "1");

  useEffect(() => {
    const st = useStore.getState();
    void st.init();
    const un = listen<AgentEvent>("agent", (e) => useStore.getState().handleEvent(e.payload));
    return () => void un.then((f) => f());
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const st = useStore.getState();
      const mod = e.ctrlKey || e.metaKey;
      if (!mod) return;
      const k = e.key.toLowerCase();
      if (k === "k") {
        e.preventDefault();
        st.setModal(st.modal === "palette" ? null : "palette");
      } else if (k === "n") {
        e.preventDefault();
        void st.newChat();
      } else if (k === ",") {
        e.preventDefault();
        st.setModal("settings");
      } else if (k === "\\") {
        e.preventDefault();
        st.togglePanel();
      } else if (k === "`") {
        e.preventDefault();
        st.setPanel("terminal");
      } else if (k === "o") {
        e.preventDefault();
        void open({ directory: true }).then((d) => {
          if (typeof d === "string") void st.openProject(d);
        });
      }
    };
    // External links open in the system browser, never inside the app window.
    const onClick = (e: MouseEvent) => {
      const a = (e.target as HTMLElement).closest?.("a[href]") as HTMLAnchorElement | null;
      if (a && /^https?:/i.test(a.href)) {
        e.preventDefault();
        void openUrl(a.href);
      }
    };
    window.addEventListener("keydown", onKey);
    document.addEventListener("click", onClick);
    return () => {
      window.removeEventListener("keydown", onKey);
      document.removeEventListener("click", onClick);
    };
  }, []);

  if (!ready) return <div className="boot" />;

  // Local providers (Ollama, custom endpoints) count as connected without a key.
  const hasProvider = providers.some((p) => p.has_key) || ["ollama", "custom"].includes(settings?.provider ?? "");
  if (!onboarded && (!hasProvider || !project)) {
    return (
      <Onboarding
        onDone={() => {
          localStorage.setItem("onboarded", "1");
          setOnboarded(true);
        }}
      />
    );
  }

  return (
    <div className={`app ${panelOpen ? "with-panel" : ""}`}>
      <Rail />
      <Chat />
      {panelOpen && <ContextPanel />}
      {modal === "settings" && <SettingsModal />}
      {modal === "skills" && <SkillsModal />}
      {modal === "palette" && <Palette />}
      <UpdateBanner />
      {toast && <div className="toast">{toast}</div>}
    </div>
  );
}
