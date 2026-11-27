// In-app updates. New versions are published to GitHub Releases by
// .github/workflows/release.yml; the Tauri updater reads latest.json from
// the latest release and verifies its signature before installing.
import { create } from "zustand";
import { getVersion } from "@tauri-apps/api/app";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

type Status = "idle" | "checking" | "available" | "none" | "downloading" | "error";

interface UpdateStore {
  current: string;
  status: Status;
  update: Update | null;
  progress: number; // 0..1 while downloading
  error: string | null;
  dismissed: boolean;
  check(manual?: boolean): Promise<void>;
  install(): Promise<void>;
  dismiss(): void;
}

export const useUpdates = create<UpdateStore>((set, get) => ({
  current: "",
  status: "idle",
  update: null,
  progress: 0,
  error: null,
  dismissed: false,

  async check(manual = false) {
    if (get().status === "checking" || get().status === "downloading") return;
    set({ status: "checking", error: null });
    try {
      if (!get().current) set({ current: await getVersion() });
      const update = await check();
      set({ update, status: update ? "available" : "none", dismissed: manual ? false : get().dismissed });
    } catch (e) {
      // Offline or no release yet: stay quiet unless the user asked.
      set({ status: manual ? "error" : "idle", error: String(e) });
    }
  },

  async install() {
    const update = get().update;
    if (!update) return;
    set({ status: "downloading", progress: 0, error: null });
    try {
      let total = 0;
      let done = 0;
      await update.downloadAndInstall((ev) => {
        if (ev.event === "Started") total = ev.data.contentLength ?? 0;
        else if (ev.event === "Progress") {
          done += ev.data.chunkLength;
          if (total) set({ progress: Math.min(1, done / total) });
        } else if (ev.event === "Finished") set({ progress: 1 });
      });
      await relaunch();
    } catch (e) {
      set({ status: "error", error: String(e) });
    }
  },

  dismiss() {
    set({ dismissed: true });
  },
}));
