import { EditorView } from "@codemirror/view";
import { HighlightStyle } from "@codemirror/language";
import { tags } from "@lezer/highlight";

export const editorTheme = EditorView.theme({
  "&": {
    height: "100%",
    color: "var(--text-strong)",
    backgroundColor: "var(--bg)",
    fontSize: "13px",
  },
  "&.cm-focused": {
    outline: "none",
  },
  ".cm-scroller": {
    fontFamily: "var(--font-mono)",
    lineHeight: "1.6",
  },
  ".cm-content": {
    padding: "6px 0",
    caretColor: "var(--text-strong)",
  },
  ".cm-cursor, .cm-dropCursor": {
    borderLeftColor: "var(--text-strong)",
    borderLeftWidth: "2px",
  },
  "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection": {
    backgroundColor: "#214283",
  },
  // Translucent because the selection layer is drawn underneath the lines.
  ".cm-activeLine": {
    backgroundColor: "rgb(255 255 255 / 6%)",
  },
  ".cm-activeStatement": {
    backgroundColor: "rgb(74 134 240 / 8%)",
  },
  ".cm-activeStatement.cm-activeLine": {
    backgroundColor: "rgb(74 134 240 / 14%)",
  },
  ".cm-selectionMatch": {
    backgroundColor: "#373b39",
  },
  "&.cm-focused .cm-matchingBracket": {
    backgroundColor: "#43454a",
    color: "var(--text-strong)",
  },
  ".cm-gutters": {
    backgroundColor: "var(--bg)",
    color: "var(--text-faint)",
    border: "none",
  },
  ".cm-lineNumbers .cm-gutterElement": {
    padding: "0 10px 0 14px",
    minWidth: "40px",
  },
  ".cm-activeLineGutter": {
    backgroundColor: "transparent",
    color: "var(--text-muted)",
  },
  ".cm-foldGutter .cm-gutterElement": {
    color: "var(--text-faint)",
  },
  ".cm-tooltip": {
    border: "1px solid var(--border-strong)",
    borderRadius: "var(--radius-md)",
    backgroundColor: "var(--surface-raised)",
    boxShadow: "var(--shadow-float)",
    overflow: "hidden",
  },
  ".cm-tooltip-autocomplete > ul": {
    fontFamily: "var(--font-mono)",
    fontSize: "12px",
    maxHeight: "260px",
  },
  ".cm-tooltip-autocomplete > ul > li": {
    padding: "3px 10px 3px 6px",
  },
  ".cm-tooltip-autocomplete > ul > li[aria-selected]": {
    backgroundColor: "var(--accent)",
    color: "#fff",
  },
  ".cm-completionDetail": {
    color: "var(--text-faint)",
    fontStyle: "normal",
  },
  ".cm-lintRange-error": {
    backgroundImage: "none",
    textDecoration: "underline wavy var(--danger)",
    textDecorationSkipInk: "none",
    textUnderlineOffset: "3px",
  },
  ".cm-tooltip-hover, .cm-tooltip-autocomplete": {
    animation: "editor-tooltip-in var(--duration) var(--ease)",
  },
  ".cm-tooltip.cm-tooltip-leaving": {
    animation: "editor-tooltip-out var(--duration-fast) var(--ease) forwards",
    pointerEvents: "none",
  },
  ".cm-tooltip-lint": {
    padding: "0",
  },
  ".cm-diagnostic": {
    padding: "5px 9px",
    fontFamily: "var(--font-sans)",
    fontSize: "12px",
  },
  ".cm-diagnostic-error": {
    borderLeft: "3px solid var(--danger)",
  },
  ".cm-panels": {
    backgroundColor: "var(--surface)",
    color: "var(--text)",
  },
  ".cm-panels.cm-panels-bottom": {
    borderTop: "1px solid var(--border)",
  },
  ".cm-searchMatch": {
    backgroundColor: "rgb(240 180 76 / 25%)",
  },
  ".cm-searchMatch.cm-searchMatch-selected": {
    backgroundColor: "rgb(240 180 76 / 45%)",
  },
  ".cm-textfield": {
    backgroundColor: "var(--control)",
    border: "1px solid var(--control-border)",
    borderRadius: "var(--radius-sm)",
    color: "var(--text-strong)",
  },
  ".cm-button": {
    backgroundImage: "none",
    backgroundColor: "var(--control)",
    border: "1px solid var(--control-border)",
    borderRadius: "var(--radius-sm)",
    color: "var(--text)",
  },
}, { dark: true });

export const highlightStyle = HighlightStyle.define([
  { tag: tags.keyword, color: "#cf8e6d" },
  { tag: [tags.typeName, tags.standard(tags.name)], color: "#cf8e6d" },
  { tag: [tags.string, tags.special(tags.string)], color: "#6aab73" },
  { tag: [tags.number, tags.bool, tags.null], color: "#2aacb8" },
  { tag: [tags.lineComment, tags.blockComment, tags.comment], color: "#7a7e85", fontStyle: "italic" },
  { tag: [tags.operator, tags.punctuation, tags.bracket], color: "#bcbec4" },
  { tag: tags.special(tags.name), color: "#c77dbb" },
  { tag: [tags.name, tags.propertyName], color: "#bcbec4" },
  { tag: tags.quote, color: "#6aab73" },
  { tag: tags.invalid, color: "var(--danger)" },
]);
