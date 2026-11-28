import { useEffect, useState } from "react";
import { ArrowLeft, Braces, ChevronRight, Download, Pencil, Plus, RotateCw, ScrollText, Trash2, Wrench } from "lucide-react";
import { api, type McpServer } from "../lib/api";
import { useStore } from "../store";
import { CodeView } from "./Code";

type Kind = "stdio" | "http" | "sse";

interface Form {
  previous?: string;
  name: string;
  kind: Kind;
  command: string;
  args: string;
  env: string;
  url: string;
  headers: string;
  autoApprove: boolean;
  /** Keys from the config we don't edit here, kept as they were. */
  rest: Record<string, unknown>;
}

const EXAMPLE = `{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/folder"]
    },
    "remote-example": {
      "type": "http",
      "url": "https://example.com/mcp",
      "headers": { "Authorization": "Bearer \${MY_TOKEN}" }
    }
  }
}
`;

/** Split a command line into arguments, honouring quotes. */
export function splitArgs(s: string): string[] {
  const out: string[] = [];
  let cur = "";
  let quote: string | null = null;
  let has = false;
  for (const ch of s) {
    if (quote) {
      if (ch === quote) quote = null;
      else cur += ch;
    } else if (ch === '"' || ch === "'") {
      quote = ch;
      has = true;
    } else if (/\s/.test(ch)) {
      if (cur || has) out.push(cur);
      cur = "";
      has = false;
    } else cur += ch;
  }
  if (cur || has) out.push(cur);
  return out;
}

const joinArgs = (a: string[]) => a.map((x) => (x === "" || /[\s"']/.test(x) ? `"${x.replace(/"/g, '\\"')}"` : x)).join(" ");

const pairs = (text: string, sep: "=" | ":") => {
  const out: Record<string, string> = {};
  for (const line of text.split("\n")) {
    const i = line.indexOf(sep);
    if (i > 0) out[line.slice(0, i).trim()] = line.slice(i + 1).trim();
  }
  return out;
};

const lines = (m: unknown, sep: string) =>
  m && typeof m === "object" ? Object.entries(m as Record<string, unknown>).map(([k, v]) => `${k}${sep}${String(v)}`).join("\n") : "";

function blank(): Form {
  return { name: "", kind: "stdio", command: "", args: "", env: "", url: "", headers: "", autoApprove: false, rest: {} };
}

function formFrom(name: string, raw: Record<string, any>): Form {
  const { command, args, env, url, serverUrl, headers, type, transport, autoApprove, alwaysAllow, ...rest } = raw;
  const t = String(type ?? transport ?? "").toLowerCase();
  const kind: Kind = command ? "stdio" : t === "sse" ? "sse" : "http";
  // An auto-approve list of specific tools isn't editable here; keep it.
  if (Array.isArray(autoApprove)) rest.autoApprove = autoApprove;
  if (alwaysAllow !== undefined) rest.alwaysAllow = alwaysAllow;
  return {
    previous: name,
    name,
    kind,
    command: command ?? "",
    args: joinArgs(Array.isArray(args) ? args.map(String) : []),
    env: lines(env, "="),
    url: url ?? serverUrl ?? "",
    headers: lines(headers, ": "),
    autoApprove: autoApprove === true,
    rest,
  };
}

function toConfig(f: Form): Record<string, unknown> {
  const out: Record<string, unknown> = { ...f.rest };
  if (f.kind === "stdio") {
    out.command = f.command.trim();
    const args = splitArgs(f.args);
    if (args.length) out.args = args;
    const env = pairs(f.env, "=");
    if (Object.keys(env).length) out.env = env;
  } else {
    out.type = f.kind;
    out.url = f.url.trim();
    const headers = pairs(f.headers, ":");
    if (Object.keys(headers).length) out.headers = headers;
  }
  if (f.autoApprove) out.autoApprove = true;
  else if (out.autoApprove === true) delete out.autoApprove;
  return out;
}

async function rawServers(): Promise<Record<string, any>> {
  const { text } = await api.mcpConfig();
  try {
    const v = JSON.parse(text);
    return v.mcpServers ?? v.servers ?? {};
  } catch {
    return {};
  }
}

const STATUS_LABEL: Record<McpServer["status"], string> = {
  connected: "Connected",
  connecting: "Connecting…",
  error: "Failed",
  disabled: "Off",
};

export function McpPane({ onEditing }: { onEditing: (editing: boolean) => void }) {
  const servers = useStore((s) => s.mcpServers);
  const notify = useStore((s) => s.notify);
  const [view, setView] = useState<"list" | "json" | "form">("list");
  const [form, setForm] = useState<Form>(blank);
  const [json, setJson] = useState("");
  const [jsonPath, setJsonPath] = useState("");
  const [jsonError, setJsonError] = useState<string | null>(null);
  const [open, setOpen] = useState<Record<string, "tools" | "logs" | null>>({});
  const [logs, setLogs] = useState<Record<string, string[]>>({});

  useEffect(() => onEditing(view === "json"), [view]);
  useEffect(() => void useStore.getState().refreshMcp(), []);

  const refresh = () => useStore.getState().refreshMcp();

  const openJson = async () => {
    const { text, path } = await api.mcpConfig();
    setJson(text);
    setJsonPath(path);
    setJsonError(null);
    setView("json");
  };

  const saveJson = async () => {
    try {
      await api.mcpSaveConfig(json);
      setJsonError(null);
      notify("MCP config saved");
      await refresh();
      setView("list");
    } catch (e) {
      setJsonError(String(e));
    }
  };

  const importApps = async () => {
    try {
      const r = await api.mcpImportApps();
      if (!r.sources.length) notify("No Claude Desktop, Claude Code, Cursor or Windsurf MCP config found on this computer.");
      else if (!r.added.length) notify(`Nothing new to import from ${r.sources.join(", ")}.`);
      else notify(`Imported ${r.added.join(", ")} from ${r.sources.join(", ")}`);
      await refresh();
    } catch (e) {
      notify(String(e));
    }
  };

  const edit = async (name: string) => {
    const raw = (await rawServers())[name] ?? {};
    setForm(formFrom(name, raw));
    setView("form");
  };

  const saveForm = async () => {
    try {
      await api.mcpPutServer(form.name.trim(), toConfig(form), form.previous);
      notify(`${form.name.trim()} saved`);
      await refresh();
      setView("list");
    } catch (e) {
      notify(String(e));
    }
  };

  const showLogs = async (name: string) => {
    if (open[name] === "logs") return setOpen((o) => ({ ...o, [name]: null }));
    setLogs((l) => ({ ...l, [name]: [] }));
    setOpen((o) => ({ ...o, [name]: "logs" }));
    const got = await api.mcpLogs(name);
    setLogs((l) => ({ ...l, [name]: got }));
  };

  if (view === "json")
    return (
      <div className="skill-edit">
        <div className="skill-editor-head">
          <button className="icon-btn" title="Back" onClick={() => setView("list")}>
            <ArrowLeft size={15} />
          </button>
          <span className="skill-title">mcp.json</span>
          <span className="mono muted small json-path" title={jsonPath}>
            {jsonPath}
          </span>
          <div className="spacer" />
          <button className="btn sm" onClick={() => setJson(EXAMPLE)} title="Replace with an example">
            Example
          </button>
          <button className="btn primary sm" onClick={saveJson}>
            Save
          </button>
        </div>
        <p className="muted small json-hint">
          Paste a config from Claude Desktop, Claude Code, Cursor or VS Code. Casylum reads <span className="mono">mcpServers</span> (or <span className="mono">servers</span>), and{" "}
          <span className="mono">{"${VAR}"}</span> picks up environment variables.
        </p>
        {jsonError && <div className="form-error">{jsonError}</div>}
        <div className="skill-code">
          <CodeView key="mcp-json" path="mcp.json" value={json} onChange={setJson} />
        </div>
      </div>
    );

  if (view === "form") {
    const f = form;
    const set = (patch: Partial<Form>) => setForm({ ...f, ...patch });
    const valid = /^[A-Za-z0-9_.-]{1,48}$/.test(f.name.trim()) && (f.kind === "stdio" ? !!f.command.trim() : /^https?:\/\//.test(f.url.trim()));
    return (
      <div className="form mcp-form">
        <div className="form-title">
          <button className="icon-btn" title="Back" onClick={() => setView("list")}>
            <ArrowLeft size={15} />
          </button>
          <span className="skill-title">{f.previous ? `Edit ${f.previous}` : "Add an MCP server"}</span>
        </div>
        <label>
          <span>Name</span>
          <input className="text-input" autoFocus={!f.previous} value={f.name} placeholder="e.g. github" onChange={(e) => set({ name: e.target.value.replace(/[^A-Za-z0-9_.-]/g, "-") })} />
        </label>
        <div className="form-field">
          <span>Type</span>
          <div className="seg">
            {(
              [
                ["stdio", "Local command"],
                ["http", "Remote (HTTP)"],
                ["sse", "Remote (SSE)"],
              ] as const
            ).map(([k, l]) => (
              <button key={k} className={f.kind === k ? "on" : ""} onClick={() => set({ kind: k })}>
                {l}
              </button>
            ))}
          </div>
        </div>
        {f.kind === "stdio" ? (
          <>
            <label>
              <span>Command</span>
              <input className="text-input mono" value={f.command} placeholder="npx, uvx, docker, node, python…" onChange={(e) => set({ command: e.target.value })} />
            </label>
            <label>
              <span>Arguments</span>
              <input className="text-input mono" value={f.args} placeholder='-y @modelcontextprotocol/server-filesystem "/path/to/folder"' onChange={(e) => set({ args: e.target.value })} />
            </label>
            <label>
              <span>
                Environment variables <span className="muted">· one KEY=value per line</span>
              </span>
              <textarea className="text-input mono" rows={3} value={f.env} placeholder="GITHUB_TOKEN=ghp_…" onChange={(e) => set({ env: e.target.value })} />
            </label>
          </>
        ) : (
          <>
            <label>
              <span>URL</span>
              <input className="text-input mono" value={f.url} placeholder={f.kind === "sse" ? "https://example.com/sse" : "https://example.com/mcp"} onChange={(e) => set({ url: e.target.value })} />
            </label>
            <label>
              <span>
                Headers <span className="muted">· one Name: value per line</span>
              </span>
              <textarea className="text-input mono" rows={3} value={f.headers} placeholder="Authorization: Bearer …" onChange={(e) => set({ headers: e.target.value })} />
            </label>
          </>
        )}
        <label className="check-row">
          <input type="checkbox" checked={f.autoApprove} onChange={(e) => set({ autoApprove: e.target.checked })} />
          <span>
            Run this server's tools without asking
            <small className="muted">Tools the server marks read-only never ask. Others ask first unless this is on or the chat is in Full auto.</small>
          </span>
        </label>
        <p className="muted small">Settings are saved in mcp.json and stored on this computer as plain text, like Claude Desktop does. Prefer {"${VAR}"} for secrets you keep in your environment.</p>
        <div className="form-actions">
          <div className="spacer" />
          <button className="btn" onClick={() => setView("list")}>
            Cancel
          </button>
          <button className="btn primary" onClick={saveForm} disabled={!valid}>
            {f.previous ? "Save" : "Add server"}
          </button>
        </div>
      </div>
    );
  }

  return (
    <>
      <div className="pane-head">
        <p className="muted small">
          MCP servers give the agent more tools: GitHub, databases, browsers, docs and more. Tools from connected servers work with every provider that supports tool calling. The config uses the same format as Claude Desktop and Claude Code, so you can paste
          yours.
        </p>
        <div className="pane-actions">
          <button
            className="btn sm"
            onClick={() => {
              setForm(blank());
              setView("form");
            }}
          >
            <Plus size={13} /> Add server
          </button>
          <button className="btn sm" onClick={openJson}>
            <Braces size={13} /> Edit JSON
          </button>
          <button className="btn sm" onClick={importApps} title="Copy servers from Claude Desktop, Claude Code, Cursor and Windsurf">
            <Download size={13} /> Import from Claude…
          </button>
        </div>
      </div>
      {servers.length === 0 && (
        <div className="mcp-empty">
          <Wrench size={20} />
          <p>No MCP servers yet.</p>
          <small className="muted">Add one with the form, or open Edit JSON and paste an existing mcpServers config.</small>
        </div>
      )}
      {servers.map((s) => {
        const o = open[s.name];
        return (
          <div key={s.name} className={`mcp-card ${s.status}`}>
            <div className="mcp-head">
              <span className={`mcp-dot ${s.status}`} title={STATUS_LABEL[s.status]} />
              <div className="mcp-meta">
                <div className="provider-name">
                  {s.name}
                  <span className="mcp-kind">{s.transport === "stdio" ? "local" : s.transport}</span>
                  {s.auto_approve_all && <span className="tag">No approval</span>}
                </div>
                <div className="mono muted small mcp-target" title={s.target}>
                  {s.target}
                </div>
              </div>
              <span className={`mcp-state ${s.status}`}>
                {s.status === "connected" ? `${s.tools.length} tool${s.tools.length === 1 ? "" : "s"}` : STATUS_LABEL[s.status]}
              </span>
              <label className="switch" title={s.status === "disabled" ? "Turn on" : "Turn off"}>
                <input
                  type="checkbox"
                  checked={s.status !== "disabled"}
                  onChange={async (e) => {
                    try {
                      await api.mcpSetEnabled(s.name, e.target.checked);
                    } catch (err) {
                      notify(String(err));
                    }
                    await refresh();
                  }}
                />
                <span />
              </label>
            </div>
            {s.error && <pre className="mcp-error">{s.error}</pre>}
            <div className="mcp-actions">
              {s.tools.length > 0 && (
                <button className="link-btn" onClick={() => setOpen((x) => ({ ...x, [s.name]: o === "tools" ? null : "tools" }))}>
                  <ChevronRight size={12} className={`step-chev ${o === "tools" ? "open" : ""}`} /> Tools
                </button>
              )}
              <button className="link-btn" onClick={() => void showLogs(s.name)}>
                <ScrollText size={12} /> Logs
              </button>
              <div className="spacer" />
              {s.status !== "disabled" && (
                <button className="icon-btn sm" title="Reconnect" onClick={() => void api.mcpRestart(s.name).then(refresh)}>
                  <RotateCw size={13} />
                </button>
              )}
              <button className="icon-btn sm" title="Edit" onClick={() => void edit(s.name)}>
                <Pencil size={13} />
              </button>
              <button
                className="icon-btn sm"
                title="Remove server"
                onClick={async () => {
                  await api.mcpRemoveServer(s.name);
                  notify(`Removed ${s.name}`);
                  await refresh();
                }}
              >
                <Trash2 size={13} />
              </button>
            </div>
            {o === "tools" && (
              <ul className="mcp-tools">
                {s.tools.map((t) => (
                  <li key={t.name}>
                    <span className="mono">{t.name}</span>
                    {t.read_only && <span className="mcp-ro">read-only</span>}
                    <small className="muted">{t.description}</small>
                  </li>
                ))}
              </ul>
            )}
            {o === "logs" && <pre className="mcp-logs">{(logs[s.name] ?? []).join("\n") || "No output yet."}</pre>}
          </div>
        );
      })}
    </>
  );
}
