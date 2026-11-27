import { useEffect, useRef } from "react";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { listen } from "@tauri-apps/api/event";
import "@xterm/xterm/css/xterm.css";
import { api } from "../lib/api";

function cssVar(name: string) {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

/** One shell per project, kept alive while the panel is mounted. */
export function TerminalView({ cwd, visible }: { cwd: string; visible: boolean }) {
  const host = useRef<HTMLDivElement>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const idRef = useRef<string | null>(null);
  const termRef = useRef<XTerm | null>(null);

  useEffect(() => {
    if (!host.current) return;
    const term = new XTerm({
      fontFamily: "'JetBrains Mono Variable', ui-monospace, monospace",
      fontSize: 12.5,
      lineHeight: 1.25,
      cursorBlink: true,
      allowProposedApi: true,
      theme: {
        background: cssVar("--surface"),
        foreground: cssVar("--text"),
        cursor: cssVar("--accent"),
        selectionBackground: "#f0a86844",
      },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host.current);
    fitRef.current = fit;
    termRef.current = term;
    fit.fit();

    let disposed = false;
    const unlisten: Array<() => void> = [];
    (async () => {
      const id = await api.ptySpawn(cwd, term.cols, term.rows);
      if (disposed) return void api.ptyKill(id);
      idRef.current = id;
      unlisten.push(await listen<{ id: string; data: string }>("pty-output", (e) => e.payload.id === id && term.write(e.payload.data)));
      unlisten.push(await listen<{ id: string }>("pty-exit", (e) => e.payload.id === id && term.write("\r\n\x1b[2m[process exited]\x1b[0m\r\n")));
      term.onData((d) => void api.ptyWrite(id, d));
      term.onResize(({ cols, rows }) => void api.ptyResize(id, cols, rows));
    })();

    const ro = new ResizeObserver(() => {
      try {
        fit.fit();
      } catch {
        /* hidden */
      }
    });
    ro.observe(host.current);

    return () => {
      disposed = true;
      ro.disconnect();
      unlisten.forEach((u) => u());
      if (idRef.current) void api.ptyKill(idRef.current);
      term.dispose();
    };
  }, [cwd]);

  useEffect(() => {
    if (visible)
      requestAnimationFrame(() => {
        fitRef.current?.fit();
        termRef.current?.focus();
      });
  }, [visible]);

  return <div className="terminal-host" ref={host} />;
}
