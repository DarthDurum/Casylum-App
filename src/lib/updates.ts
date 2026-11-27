// In-app updates. New versions are published to GitHub Releases by
// .github/workflows/release.yml; the Tauri updater reads latest.json from
// the latest release and verifies its signature before installing.
//
// With auto-update on (the default), a new version downloads quietly in the
// background and installs when Casylum is closed, so the next launch is on
// the new version. "Restart now" applies it right away.
import { create } from "zustand";
import { getVersion } from "@tauri-apps/api/app";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

type Status = "idle" | "checking" | "available" | "none" | "downloading" | "ready" | "installing" | "error";

/** How often a running app looks for a new release. */
const RECHECK_MS = 4 * 60 * 60 * 1000;

interface UpdateStore {
  current: string;
  status: Status;
  update: Update | null;
  progress: number; // 0..1 while downloading
  error: string | null;
  dismissed: boolean;
  /** Look for an update; with auto on, also download it. */
  check(opts?: { manual?: boolean; auto?: boolean }): Promise<void>;
  /** Download (if needed), install and restart now. */
  install(): Promise<void>;
  /** Install a downloaded update without restarting (used on quit). */
  installOnQuit(): Promise<void>;
  dismiss(): void;
}

export const useUpdates = create<UpdateStore>((set, get) => {
  const download = async (update: Update) => {
    set({ status: "downloading", progress: 0, error: null });
    let total = 0;
    let done = 0;
    await update.download((ev) => {
      if (ev.event === "Started") total = ev.data.contentLength ?? 0;
      else if (ev.event === "Progress") {
        done += ev.data.chunkLength;
        if (total) set({ progress: Math.min(1, done / total) });
      } else if (ev.event === "Finished") set({ progress: 1 });
    });
    set({ status: "ready" });
  };

  return {
    current: "",
    status: "idle",
    update: null,
    progress: 0,
    error: null,
    dismissed: false,

    async check({ manual = false, auto = false } = {}) {
      const st = get().status;
      if (st === "checking" || st === "downloading" || st === "ready" || st === "installing") return;
      set({ status: "checking", error: null });
      try {
        if (!get().current) set({ current: await getVersion() });
        const update = await check();
        set({ update, status: update ? "available" : "none", dismissed: manual ? false : get().dismissed });
        if (update && auto) await download(update);
      } catch (e) {
        // Offline or no release yet: stay quiet unless the user asked.
        set({ status: manual ? "error" : get().update ? "available" : "idle", error: String(e) });
      }
    },

    async install() {
      const update = get().update;
      if (!update) return;
      try {
        if (get().status !== "ready") await download(update);
        set({ status: "installing" });
        await update.install();
        await relaunch();
      } catch (e) {
        set({ status: "error", error: String(e) });
      }
    },

    async installOnQuit() {
      const update = get().update;
      if (!update || get().status !== "ready") return;
      set({ status: "installing" });
      try {
        await update.install();
      } catch (e) {
        set({ status: "error", error: String(e) });
      }
    },

    dismiss() {
      set({ dismissed: true });
    },
  };
});

let started = false;

/**
 * Start background updates: check now, then every few hours, and install a
 * downloaded update when the window closes. `isAuto` is read each time so
 * the Settings toggle takes effect without a restart.
 */
export function startAutoUpdates(isAuto: () => boolean) {
  if (started) return;
  started = true;
  const run = () => void useUpdates.getState().check({ auto: isAuto() });
  run();
  window.setInterval(run, RECHECK_MS);

  void getCurrentWindow().onCloseRequested(async (event) => {
    const st = useUpdates.getState();
    if (st.status !== "ready") return;
    // Hold the close until the installer has the new version in place.
    event.preventDefault();
    await st.installOnQuit();
    await getCurrentWindow().destroy();
  });
}
