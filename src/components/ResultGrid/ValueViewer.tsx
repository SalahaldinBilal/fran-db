import styles from "./ValueViewer.module.scss";
import { Match, Show, Switch, createComputed, createMemo, createSignal, on, onCleanup } from "solid-js";
import { Copy } from "lucide-solid";
import { ColumnChoices, ColumnMeta } from "@core/types";
import { copyText } from "@core/helpers/clipboard";
import { safeInvoke } from "@core/helpers/safeInvoke";
import { compactJson, formatJson, looksLikeJson } from "@core/helpers/values";
import Modal from "../Modal/Modal";
import Button from "../Button/Button";
import Spinner from "../Spinner/Spinner";
import SegmentedControl from "../SegmentedControl/SegmentedControl";
import CodeView, { CodeViewApi } from "../CodeView/CodeView";
import TemporalView from "./views/TemporalView";
import NumberView from "./views/NumberView";
import BinaryView from "./views/BinaryView";
import ChoiceView from "./views/ChoiceView";
import { canonicalTemporal, fractionDigitsOf, isZonedType, temporalKind } from "./views/temporal";

export type ValueViewerProps = {
  cell: { row: number, column: number } | null,
  column: ColumnMeta | null,
  value: string | null,
  editable: boolean,
  sessionId?: string,
  onSave: (value: string | null) => void,
  onClose: () => void,
};

type ViewKind = "temporal" | "number" | "binary" | "json" | "text" | "choice" | "loading";

const COMPACT_CHOICES = 10;
const SLOW_CHOICES_MS = 300;
type JsonMode = "formatted" | "compact";
type BinaryMode = "bytes" | "raw";

function viewKind(column: ColumnMeta | null, value: string | null): ViewKind {
  switch (column?.category) {
    case "temporal":
      return "temporal";
    case "integer":
    case "decimal":
    case "float":
    case "bit":
      return "number";
    case "binary":
    case "spatial":
      return "binary";
    case "json":
      return "json";
  }
  return value !== null && looksLikeJson(value) ? "json" : "text";
}

function ValueViewer(props: ValueViewerProps) {
  const [draft, setDraft] = createSignal("");
  const [jsonMode, setJsonMode] = createSignal<JsonMode>("formatted");
  const [binaryMode, setBinaryMode] = createSignal<BinaryMode>("bytes");
  const [choices, setChoices] = createSignal<ColumnChoices | null | "loading">(null);
  const [slowChoices, setSlowChoices] = createSignal(false);
  let editor: CodeViewApi | undefined;
  let choicesRequest = 0;
  let slowTimer: ReturnType<typeof setTimeout> | undefined;
  onCleanup(() => clearTimeout(slowTimer));

  const loadedChoices = createMemo(() => {
    const loaded = choices();
    return loaded && loaded !== "loading" ? loaded : null;
  });

  const width = createMemo(() => ((loadedChoices()?.values.length ?? Infinity) <= COMPACT_CHOICES ? 560 : 760));

  const kind = createMemo<ViewKind>(() => {
    const loaded = choices();
    if (loaded === "loading") return "loading";
    if (loaded) return "choice";
    return viewKind(props.column, props.value);
  });

  function loadChoices() {
    const request = ++choicesRequest;
    const column = props.column;
    const sessionId = props.sessionId;
    if (!props.cell || !column || !sessionId || !column.hasChoices
      || !column.database || !column.table || !column.sourceName) {
      setChoices(null);
      return;
    }

    setChoices("loading");
    setSlowChoices(false);
    clearTimeout(slowTimer);
    slowTimer = setTimeout(() => request === choicesRequest && setSlowChoices(true), SLOW_CHOICES_MS);
    safeInvoke("get_column_choices", { sessionId, database: column.database, table: column.table, column: column.sourceName })
      .then(
        result => request === choicesRequest && setChoices(result),
        () => request === choicesRequest && setChoices(null),
      );
  }
  const showsCode = createMemo(() => kind() === "json" || kind() === "text" || (kind() === "binary" && binaryMode() === "raw"));

  const initialText = createMemo(() => {
    const value = props.value;
    if (value === null) return "";
    if (kind() !== "json") return value;
    try {
      return formatJson(value);
    } catch {
      return value;
    }
  });

  createComputed(on(() => props.cell, () => {
    setJsonMode("formatted");
    setBinaryMode("bytes");
    loadChoices();
    setDraft(initialText());
  }));

  const subtitle = createMemo(() => {
    if (!props.cell || !props.column) return undefined;
    const parts = [`${props.column.typeName}${kind() === "json" && props.column.category !== "json" ? " (JSON)" : ""}`, `row ${props.cell.row + 1}`];
    if (kind() === "text" || kind() === "json") parts.push(`${draft().length.toLocaleString()} characters`);
    return parts.join(", ");
  });

  function switchJsonMode(next: JsonMode) {
    if (!editor || next === jsonMode()) return;
    try {
      editor.setText(next === "formatted" ? formatJson(draft()) : compactJson(draft()));
    } catch {
      return;
    }
    setJsonMode(next);
  }

  function save() {
    let text = draft();
    if (kind() === "temporal" || kind() === "number") text = text.trim();
    if (kind() === "temporal" && props.column) {
      const temporal = temporalKind(props.column.typeName);
      const zoned = isZonedType(props.column.typeName);
      text = canonicalTemporal(temporal, text, fractionDigitsOf(temporal, props.value, zoned), zoned) ?? text;
    }

    if (text === "" && props.value === null) {
      props.onClose();
      return;
    }

    const originalCompact = props.value !== null && !props.value.includes("\n");
    if (kind() === "json" && originalCompact && looksLikeJson(text)) text = compactJson(text);

    props.onSave(text);
    props.onClose();
  }

  function onKeyDown(event: KeyboardEvent) {
    if (props.editable && event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      event.stopPropagation();
      save();
    }
  }

  return (
    <Modal
      show={props.cell !== null && (kind() !== "loading" || slowChoices())}
      onHide={props.onClose}
      title={props.column?.name}
      subtitle={subtitle()}
      width={width()}
      footer={
        <>
          <Button variant="subtle" onClick={() => copyText(draft(), "value")} disabled={props.value === null && draft() === ""}>
            <Copy size={14} />
            Copy
          </Button>
          <Show when={kind() === "json"}>
            <SegmentedControl
              value={jsonMode()}
              onChange={switchJsonMode}
              options={[
                { value: "formatted", label: "Formatted" },
                { value: "compact", label: "Compact" },
              ]}
            />
          </Show>
          <Show when={kind() === "binary"}>
            <SegmentedControl
              value={binaryMode()}
              onChange={setBinaryMode}
              options={[
                { value: "bytes", label: "Bytes" },
                { value: "raw", label: "Raw" },
              ]}
            />
          </Show>
          <div class={styles.FooterSpacer} />
          <Show
            when={props.editable}
            fallback={<Button variant="primary" onClick={props.onClose}>Close</Button>}
          >
            <Show when={props.column?.nullable}>
              <Button
                variant="subtle"
                onClick={() => {
                  props.onSave(null);
                  props.onClose();
                }}
              >
                Set NULL
              </Button>
            </Show>
            <Button variant="subtle" onClick={props.onClose}>Cancel</Button>
            <Button variant="primary" onClick={save} tooltip="Ctrl+Enter">Apply</Button>
          </Show>
        </>
      }
    >
      <Show when={props.cell} keyed>
        <div ref={element => element.addEventListener("keydown", onKeyDown, true)}>
          <Show when={props.value !== null || props.editable} fallback={<div class={styles.Null}>NULL</div>}>
            <Switch>
              <Match when={showsCode()}>
                <div class={styles.CodeBody}>
                  <CodeView
                    value={draft()}
                    language={kind() === "json" ? "json" : "text"}
                    readOnly={!props.editable}
                    onChange={setDraft}
                    onReady={api => {
                      editor = api;
                      api.focus();
                    }}
                  />
                </div>
              </Match>
              <Match when={kind() === "loading"}>
                <div class={styles.Loading}><Spinner size={16} /></div>
              </Match>
              <Match when={loadedChoices()}>
                {loaded => (
                  <ChoiceView
                    choices={loaded()}
                    value={draft()}
                    original={props.value}
                    isNull={props.value === null && draft() === ""}
                    editable={props.editable}
                    onChange={setDraft}
                  />
                )}
              </Match>
              <Match when={kind() === "temporal"}>
                <TemporalView column={props.column!} value={draft()} editable={props.editable} onChange={setDraft} />
              </Match>
              <Match when={kind() === "number"}>
                <NumberView column={props.column!} value={draft()} editable={props.editable} onChange={setDraft} />
              </Match>
              <Match when={kind() === "binary"}>
                <div class={styles.CodeBody}>
                  <BinaryView value={draft()} />
                </div>
              </Match>
            </Switch>
          </Show>
        </div>
      </Show>
    </Modal>
  );
}

export default ValueViewer;
