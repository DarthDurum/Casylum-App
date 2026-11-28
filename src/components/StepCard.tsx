import { useState } from "react";
import {
  BookOpen, Check, ChevronRight, FilePen, FilePlus2, FolderTree, ListChecks, Loader2, Search, Sparkles, SquareTerminal, X, ShieldQuestion, Plug,
} from "lucide-react";
import type { ToolResult, ToolUse } from "../lib/steps";
import { stepLabel } from "../lib/steps";
import { useStore } from "../store";
import type { Approval } from "../store";
import { DiffView } from "./Code";

const ICONS: Record<string, any> = {
  read_file: BookOpen,
  list_dir: FolderTree,
  search: Search,
  write_file: FilePlus2,
  edit_file: FilePen,
  run_command: SquareTerminal,
  update_plan: ListChecks,
  use_skill: Sparkles,
};

export function StepCard({ tool, result, active, approval }: { tool: ToolUse; result?: ToolResult; active: boolean; approval?: Approval }) {
  const [open, setOpen] = useState(false);
  const decide = useStore((s) => s.decide);
  const showFile = useStore((s) => s.showFile);
  const Icon = ICONS[tool.name] ?? (tool.name.startsWith("mcp__") ? Plug : Sparkles);
  const { verb, target, extra } = stepLabel(tool, result);
  const status = approval ? "ask" : result ? (result.is_error ? "error" : "done") : active ? "running" : "queued";
  const isDiff = result?.meta?.kind === "diff";

  return (
    <div className={`step step-${status}`}>
      <button className="step-row" onClick={() => setOpen((o) => !o)} disabled={!result && !approval}>
        <span className="step-icon">
          <Icon size={14} />
        </span>
        <span className="step-verb">{verb}</span>
        <span className="step-target">{target}</span>
        {extra && <span className={`step-extra ${isDiff ? "diffstat" : ""}`}>{extra}</span>}
        <span className="step-status">
          {status === "running" && <Loader2 size={13} className="spin" />}
          {status === "done" && <Check size={13} />}
          {status === "error" && <X size={13} />}
          {status === "ask" && <ShieldQuestion size={14} />}
        </span>
        {(result || approval) && <ChevronRight size={13} className={`step-chev ${open || approval ? "open" : ""}`} />}
      </button>

      {approval && (
        <div className="step-approval">
          <ApprovalPreview approval={approval} />
          <div className="approval-actions">
            <button className="btn primary sm" onClick={() => decide(tool.id, "allow")}>
              Allow
            </button>
            <button className="btn sm" onClick={() => decide(tool.id, "allow_always")}>
              Always allow {tool.name === "run_command" ? "commands" : "edits"} in this run
            </button>
            <button className="btn ghost sm" onClick={() => decide(tool.id, "deny")}>
              Deny
            </button>
          </div>
        </div>
      )}

      {open && result && !approval && (
        <div className="step-body">
          {isDiff && result.meta.after != null ? (
            <>
              <div className="diff-wrap">
                <DiffView path={result.meta.path} before={result.meta.before ?? ""} after={result.meta.after} />
              </div>
              <button className="link-btn" onClick={() => showFile(result.meta.path)}>
                Open in Changes
              </button>
            </>
          ) : (
            <pre className="step-output">{result.content}</pre>
          )}
        </div>
      )}
    </div>
  );
}

function ApprovalPreview({ approval }: { approval: Approval }) {
  const i = approval.input ?? {};
  if (approval.name === "run_command") return <pre className="step-output cmd">$ {i.command}</pre>;
  if (approval.name === "edit_file")
    return (
      <div className="diff-wrap">
        <DiffView path={i.path ?? ""} before={i.old_string ?? ""} after={i.new_string ?? ""} />
      </div>
    );
  if (approval.name === "write_file")
    return (
      <div className="diff-wrap">
        <DiffView path={i.path ?? ""} before="" after={i.content ?? ""} />
      </div>
    );
  return <pre className="step-output">{JSON.stringify(i, null, 2)}</pre>;
}
