import styles from "./SqlEditor.module.scss";
import { createEffect, on, onCleanup, onMount } from "solid-js";
import { Compartment, EditorState, Extension, Prec, RangeSetBuilder, StateEffect, StateField } from "@codemirror/state";
import {
  Decoration, DecorationSet, EditorView, crosshairCursor, drawSelection, dropCursor, highlightActiveLine,
  highlightActiveLineGutter, highlightSpecialChars, keymap, lineNumbers, rectangularSelection,
} from "@codemirror/view";
import { TextRange, statementAt } from "@core/helpers/statements";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { SQLDialect, SQLNamespace, sql } from "@codemirror/lang-sql";
import { acceptCompletion, autocompletion, closeBrackets, closeBracketsKeymap, completionKeymap } from "@codemirror/autocomplete";
import { bracketMatching, foldGutter, foldKeymap, indentOnInput, syntaxHighlighting } from "@codemirror/language";
import { highlightSelectionMatches, searchKeymap } from "@codemirror/search";
import { forceLinting } from "@codemirror/lint";
import { editorTheme, highlightStyle } from "./theme";
import { SchemaCatalog, schemaLinter } from "./schemaLinter";
import { fadeOutHoverTooltips } from "./tooltipFade";

export type SqlEditorApi = {
  text: () => string,
  selection: () => { from: number, to: number, text: string },
  cursor: () => number,
  focus: () => void,
  setText: (text: string) => void,
};

export type SqlEditorProps = {
  initialValue: string,
  dialect: SQLDialect,
  schema?: SQLNamespace,
  defaultSchema?: string,
  catalog?: SchemaCatalog | null,
  readOnly?: boolean,
  wrap?: boolean,
  statements?: TextRange[],
  onChange?: (text: string) => void,
  onRunCurrent?: () => void,
  onRunAll?: () => void,
  onSave?: () => void,
  onReady?: (api: SqlEditorApi) => void,
};

const setStatements = StateEffect.define<TextRange[]>();

// Ranges come from the driver's splitter after a short delay, so they follow edits until fresh ones arrive.
const statementsField = StateField.define<TextRange[]>({
  create: () => [],
  update(ranges, transaction) {
    for (const effect of transaction.effects) {
      if (effect.is(setStatements)) return effect.value;
    }
    if (!transaction.docChanged) return ranges;
    return ranges.map(range => ({
      start: transaction.changes.mapPos(range.start, 1),
      end: transaction.changes.mapPos(range.end, -1),
    }));
  },
});

const activeStatementLine = Decoration.line({ class: "cm-activeStatement" });

function activeStatementDecorations(state: EditorState): DecorationSet {
  const ranges = state.field(statementsField);
  const selection = state.selection.main;
  if (ranges.length < 2 || !selection.empty) return Decoration.none;

  const range = ranges[statementAt(ranges, selection.head)];
  const builder = new RangeSetBuilder<Decoration>();
  const last = state.doc.lineAt(Math.min(range.end, state.doc.length)).number;
  for (let line = state.doc.lineAt(Math.min(range.start, state.doc.length)).number; line <= last; line++) {
    builder.add(state.doc.line(line).from, state.doc.line(line).from, activeStatementLine);
  }
  return builder.finish();
}

const activeStatement: Extension = [
  statementsField,
  EditorView.decorations.compute([statementsField, "selection"], activeStatementDecorations),
];

function SqlEditor(props: SqlEditorProps) {
  let container!: HTMLDivElement;
  let view: EditorView | undefined;
  const language = new Compartment();

  function languageExtension() {
    return sql({
      dialect: props.dialect,
      schema: props.schema,
      defaultSchema: props.defaultSchema,
      upperCaseKeywords: true,
    });
  }

  onMount(() => {
    const readOnly = props.readOnly ?? false;

    const commandKeys = Prec.highest(keymap.of([
      { key: "Mod-Enter", run: () => { props.onRunCurrent?.(); return true; } },
      { key: "Shift-Mod-Enter", run: () => { props.onRunAll?.(); return true; } },
      { key: "F5", run: () => { props.onRunAll?.(); return true; } },
      { key: "Mod-s", run: () => { props.onSave?.(); return true; } },
      { key: "Tab", run: acceptCompletion },
    ]));

    const editing: Extension[] = readOnly ? [] : [
      commandKeys,
      history(),
      indentOnInput(),
      closeBrackets(),
      autocompletion(),
      activeStatement,
      schemaLinter(() => props.catalog ?? null),
      EditorView.updateListener.of(update => {
        if (update.docChanged) props.onChange?.(update.state.doc.toString());
      }),
    ];

    view = new EditorView({
      parent: container,
      state: EditorState.create({
        doc: props.initialValue,
        extensions: [
          ...editing,
          lineNumbers(),
          highlightActiveLineGutter(),
          highlightSpecialChars(),
          foldGutter(),
          drawSelection(),
          dropCursor(),
          EditorState.allowMultipleSelections.of(true),
          syntaxHighlighting(highlightStyle),
          bracketMatching(),
          rectangularSelection(),
          crosshairCursor(),
          highlightActiveLine(),
          highlightSelectionMatches(),
          keymap.of([
            ...closeBracketsKeymap,
            ...defaultKeymap,
            ...searchKeymap,
            ...historyKeymap,
            ...foldKeymap,
            ...completionKeymap,
            indentWithTab,
          ]),
          language.of(languageExtension()),
          props.wrap ? EditorView.lineWrapping : [],
          EditorState.readOnly.of(readOnly),
          editorTheme,
        ],
      }),
    });

    const editor = view;
    onCleanup(fadeOutHoverTooltips(editor));

    props.onReady?.({
      text: () => editor.state.doc.toString(),
      selection: () => {
        const range = editor.state.selection.main;
        return { from: range.from, to: range.to, text: editor.state.sliceDoc(range.from, range.to) };
      },
      cursor: () => editor.state.selection.main.head,
      focus: () => editor.focus(),
      setText: text => editor.dispatch({ changes: { from: 0, to: editor.state.doc.length, insert: text } }),
    });
  });

  createEffect(on(() => [props.schema, props.defaultSchema, props.dialect], () => {
    view?.dispatch({ effects: language.reconfigure(languageExtension()) });
  }, { defer: true }));

  createEffect(on(() => props.statements, ranges => {
    if (view && !props.readOnly) view.dispatch({ effects: setStatements.of(ranges ?? []) });
  }));

  createEffect(on(() => props.catalog, () => {
    if (view && !props.readOnly) forceLinting(view);
  }, { defer: true }));

  onCleanup(() => view?.destroy());

  return <div ref={container} class={styles.Editor} />;
}

export default SqlEditor;
