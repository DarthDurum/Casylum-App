import { EditorView } from "@codemirror/view";
import { HighlightStyle, syntaxHighlighting, type LanguageSupport } from "@codemirror/language";
import { tags as t } from "@lezer/highlight";
import type { Extension } from "@codemirror/state";
import { javascript } from "@codemirror/lang-javascript";
import { python } from "@codemirror/lang-python";
import { rust } from "@codemirror/lang-rust";
import { json } from "@codemirror/lang-json";
import { css } from "@codemirror/lang-css";
import { html } from "@codemirror/lang-html";
import { markdown } from "@codemirror/lang-markdown";

/** Editor chrome follows the app's CSS variables, so it switches with the theme. */
export const casylumTheme: Extension = [
  EditorView.theme({
    "&": { backgroundColor: "transparent", color: "var(--text)", fontSize: "12.5px", height: "100%" },
    ".cm-content": { fontFamily: "var(--mono)", caretColor: "var(--accent)", padding: "10px 0" },
    ".cm-scroller": { fontFamily: "var(--mono)", lineHeight: "1.6" },
    ".cm-gutters": { backgroundColor: "transparent", color: "var(--muted)", border: "none", paddingLeft: "6px" },
    ".cm-activeLine, .cm-activeLineGutter": { backgroundColor: "color-mix(in srgb, var(--text) 4%, transparent)" },
    "&.cm-focused": { outline: "none" },
    ".cm-selectionBackground, &.cm-focused .cm-selectionBackground, ::selection": {
      backgroundColor: "color-mix(in srgb, var(--accent) 28%, transparent) !important",
    },
    ".cm-changedLine": { backgroundColor: "color-mix(in srgb, var(--mint) 9%, transparent) !important" },
    ".cm-changedText": { background: "color-mix(in srgb, var(--mint) 26%, transparent) !important" },
    ".cm-deletedChunk": { backgroundColor: "color-mix(in srgb, var(--rose) 10%, transparent) !important" },
    ".cm-deletedChunk .cm-deletedText, .cm-deletedText": { background: "color-mix(in srgb, var(--rose) 28%, transparent) !important" },
    ".cm-insertedLine": { backgroundColor: "color-mix(in srgb, var(--mint) 9%, transparent) !important" },
    ".cm-collapsedLines": { background: "var(--raised)", color: "var(--muted)", fontFamily: "var(--sans)" },
  }),
  syntaxHighlighting(
    HighlightStyle.define([
      { tag: [t.keyword, t.controlKeyword, t.moduleKeyword], color: "var(--syn-keyword)" },
      { tag: [t.string, t.special(t.string), t.regexp], color: "var(--syn-string)" },
      { tag: [t.number, t.bool, t.null, t.atom], color: "var(--syn-number)" },
      { tag: [t.comment, t.lineComment, t.blockComment], color: "var(--muted)", fontStyle: "italic" },
      { tag: [t.function(t.variableName), t.function(t.propertyName)], color: "var(--syn-fn)" },
      { tag: [t.typeName, t.className, t.namespace], color: "var(--syn-type)" },
      { tag: [t.propertyName, t.attributeName], color: "var(--syn-prop)" },
      { tag: [t.tagName, t.heading], color: "var(--syn-keyword)", fontWeight: "600" },
      { tag: [t.operator, t.punctuation], color: "var(--text-2)" },
      { tag: t.link, color: "var(--sky)", textDecoration: "underline" },
      { tag: t.emphasis, fontStyle: "italic" },
      { tag: t.strong, fontWeight: "700" },
    ]),
  ),
];

export function languageFor(path: string): LanguageSupport | null {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  switch (ext) {
    case "ts":
    case "mts":
    case "cts":
      return javascript({ typescript: true });
    case "tsx":
      return javascript({ typescript: true, jsx: true });
    case "js":
    case "mjs":
    case "cjs":
      return javascript();
    case "jsx":
      return javascript({ jsx: true });
    case "py":
      return python();
    case "rs":
      return rust();
    case "json":
      return json();
    case "css":
    case "scss":
      return css();
    case "html":
    case "htm":
    case "vue":
    case "svelte":
      return html();
    case "md":
    case "mdx":
      return markdown();
    default:
      return null;
  }
}
