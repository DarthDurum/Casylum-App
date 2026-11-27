import { useEffect, useRef } from "react";
import { EditorView, lineNumbers, highlightActiveLine, keymap } from "@codemirror/view";
import { EditorState } from "@codemirror/state";
import { unifiedMergeView } from "@codemirror/merge";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { casylumTheme, languageFor } from "../lib/editor";

/** Read-only code viewer, or an editor when `onChange` is given. */
export function CodeView({ path, value, onChange }: { path: string; value: string; onChange?: (v: string) => void }) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const cb = useRef(onChange);
  cb.current = onChange;

  useEffect(() => {
    if (!host.current) return;
    const lang = languageFor(path);
    const editable = !!cb.current;
    const v = new EditorView({
      parent: host.current,
      state: EditorState.create({
        doc: value,
        extensions: [
          lineNumbers(),
          casylumTheme,
          EditorView.lineWrapping,
          EditorState.readOnly.of(!editable),
          EditorView.editable.of(editable),
          ...(editable ? [history(), highlightActiveLine(), keymap.of([...defaultKeymap, ...historyKeymap])] : []),
          ...(lang ? [lang] : []),
          EditorView.updateListener.of((u) => {
            if (u.docChanged) cb.current?.(u.state.doc.toString());
          }),
        ],
      }),
    });
    view.current = v;
    return () => v.destroy();
    // Recreate only when the file changes; edits flow out through onChange.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [path]);

  useEffect(() => {
    const v = view.current;
    if (v && !cb.current && v.state.doc.toString() !== value) {
      v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: value } });
    }
  }, [value]);

  return <div className="code-host" ref={host} />;
}

/** Unified (inline) diff: compact enough for a side panel. */
export function DiffView({ path, before, after }: { path: string; before: string; after: string }) {
  const host = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!host.current) return;
    const lang = languageFor(path);
    const v = new EditorView({
      parent: host.current,
      state: EditorState.create({
        doc: after,
        extensions: [
          lineNumbers(),
          casylumTheme,
          EditorState.readOnly.of(true),
          EditorView.editable.of(false),
          unifiedMergeView({ original: before, mergeControls: false, highlightChanges: true, collapseUnchanged: { margin: 3, minSize: 6 } }),
          ...(lang ? [lang] : []),
        ],
      }),
    });
    return () => v.destroy();
  }, [path, before, after]);
  return <div className="code-host" ref={host} />;
}
