import styles from "./ConnectionEditor.module.scss";
import { For, Show, createEffect, createMemo, createSignal, on } from "solid-js";
import { createStore, produce, reconcile, unwrap } from "solid-js/store";
import { CircleAlert, CircleCheck, Pipette, Plug, Wand2 } from "lucide-solid";
import { ConnectionProfile, DatabaseKind, DriverOptions } from "@core/types";
import { databaseModule, databaseModules, defaultDriver, parseConnectionString, ParsedDriver, withDatabaseModule } from "@core/databases";
import { PendingSecret, SecretEditor } from "@core/databases/types";
import { ConnectionStringError } from "@core/databases/shared/connectionString";
import { safeInvoke } from "@core/helpers/safeInvoke";
import { withConnectPrompts } from "@core/helpers/connectFlow";
import { errorMessage } from "@core/helpers/errors";
import useConnectionsState from "@core/states/connectionsState";
import usePromptState from "@core/states/promptState";
import useToastState from "@core/states/toastState";
import Modal from "@core/components/Modal/Modal";
import Button from "@core/components/Button/Button";
import Field, { FieldRow } from "@core/components/Field/Field";
import Input from "@core/components/Input/Input";
import Select from "@core/components/Select/Select";
import SuggestInput from "@core/components/SuggestInput/SuggestInput";
import Spinner from "@core/components/Spinner/Spinner";
import ColorPicker from "@core/components/ColorPicker/ColorPicker";
import { tooltip } from "@core/components/Tooltip/Tooltip";

const COLORS = ["#4a88c7", "#4fa37a", "#c9a34a", "#d07a3c", "#c75450", "#9a6fc4", "#7d8590"];

export type ConnectionEditorProps = {
  show: boolean,
  profile: ConnectionProfile | null,
  onClose: () => void,
};

type TestResult = { ok: boolean, message: string };
type ParseNote = { ok: boolean, message: string };

// What a paste into the Host field has to look like before it's read as a connection string.
const CONNECTION_STRING_SHAPE = /^\s*([a-z][\w+.-]*:\/\/|jdbc:|\w+\s*=)/i;

function parsedSummary(parsed: ParsedDriver): string {
  const parts = [`Filled in ${parsed.filled.join(", ")}.`];
  if (parsed.password !== null) parts.push("The password goes to the vault when you save, not into the connection.");
  if (parsed.ignored.length > 0) parts.push(`Not used: ${parsed.ignored.join(", ")}.`);
  return parts.join(" ");
}

function blankProfile(kind: DatabaseKind): ConnectionProfile {
  return { id: "", name: "", color: null, group: null, forwardPort: null, agentAccess: { mode: "off", databases: [] }, ...defaultDriver(kind) };
}

function ConnectionEditor(props: ConnectionEditorProps) {
  const connections = useConnectionsState;
  const { confirm } = usePromptState;
  const { pushToast, pushError } = useToastState;
  const [connectionString, setConnectionString] = createSignal("");
  const [parseNote, setParseNote] = createSignal<ParseNote | null>(null);

  const [draft, setDraft] = createStore<ConnectionProfile>(blankProfile("mysql"));
  const [pending, setPending] = createStore<Record<string, PendingSecret>>({});
  const [storedSlots, setStoredSlots] = createSignal<string[]>([]);
  const [pickerAnchor, setPickerAnchor] = createSignal<HTMLElement | null>(null);
  const customColor = createMemo(() => draft.color !== null && !COLORS.includes(draft.color) ? draft.color : null);
  const [saving, setSaving] = createSignal(false);
  const [testing, setTesting] = createSignal(false);
  const [testResult, setTestResult] = createSignal<TestResult | null>(null);

  const isNew = createMemo(() => !draft.id);
  const target = createMemo(() => withDatabaseModule(draft, (module, options) => module.describeTarget(options)));
  const suggestedName = createMemo(() => withDatabaseModule(draft, (module, options) => module.suggestName(options)));
  const groups = createMemo(() => {
    const names = new Set(connections.groupOrder().filter(name => name !== ""));
    for (const profile of connections.connections) {
      if (profile.group !== null) names.add(profile.group);
    }
    return [...names].sort((a, b) => a.localeCompare(b));
  });

  createEffect(on(() => [props.show, props.profile] as const, ([show, profile]) => {
    if (!show) return;

    const initial: ConnectionProfile = profile ? JSON.parse(JSON.stringify(unwrap(profile))) : blankProfile("mysql");
    setDraft(reconcile(initial));
    setPending(reconcile({}));
    setStoredSlots([]);
    setTestResult(null);
    setConnectionString("");
    setParseNote(null);

    if (profile) {
      safeInvoke("get_stored_secrets", { connectionId: profile.id })
        .then(setStoredSlots)
        .catch(() => setStoredSlots([]));
    }
  }));

  const secretEditor: SecretEditor = {
    isStored: slot => storedSlots().includes(slot),
    pending: slot => pending[slot],
    setPending: (slot, value) => setPending(slot, value),
  };

  function snapshot(): ConnectionProfile {
    const copy: ConnectionProfile = JSON.parse(JSON.stringify(unwrap(draft)));
    copy.name = copy.name.trim() || suggestedName();
    return copy;
  }

  function typedSecrets(): Record<string, string> {
    const values: Record<string, string> = {};
    for (const [slot, value] of Object.entries(pending)) {
      if (typeof value === "string") values[slot] = value;
    }
    return values;
  }

  function changeKind(kind: DatabaseKind) {
    if (kind === draft.kind) return;
    setDraft(produce(profile => Object.assign(profile, defaultDriver(kind))));
    setPending(reconcile({}));
  }

  function updateOptions(mutate: (options: DriverOptions["options"]) => void) {
    setDraft("options", produce(mutate));
  }

  function hasFormChanges(): boolean {
    if (!isNew()) return true;
    return JSON.stringify(unwrap(draft.options)) !== JSON.stringify(defaultDriver(draft.kind).options);
  }

  async function applyParsed(parsed: ParsedDriver) {
    const switching = parsed.driver.kind !== draft.kind;
    if (switching && hasFormChanges()) {
      const from = databaseModule(draft.kind).label;
      const to = databaseModule(parsed.driver.kind).label;
      const confirmed = await confirm({
        title: `Switch to ${to}?`,
        message: `This is a ${to} connection string. Switching replaces the ${from} settings you've filled in.`,
        confirmLabel: `Switch to ${to}`,
      });
      if (!confirmed) return;
    }

    setDraft(produce(profile => Object.assign(profile, parsed.driver)));
    if (switching) setPending(reconcile({}));
    if (parsed.password !== null) setPending("password", parsed.password);
    setConnectionString("");
    setTestResult(null);
    setParseNote({ ok: true, message: parsedSummary(parsed) });
  }

  // True when the text was taken as a connection string, including one that turned out to be broken.
  function takeConnectionString(text: string): boolean {
    let parsed: ParsedDriver | null;
    try {
      parsed = parseConnectionString(text, unwrap(draft));
    } catch (error) {
      if (!(error instanceof ConnectionStringError)) throw error;
      setParseNote({ ok: false, message: error.message });
      return true;
    }
    if (!parsed) return false;
    void applyParsed(parsed);
    return true;
  }

  function fillFromConnectionString() {
    if (!takeConnectionString(connectionString())) {
      setParseNote({ ok: false, message: "That isn't a connection string Fran DB can read." });
    }
  }

  function pastedIntoHost(text: string): boolean {
    return CONNECTION_STRING_SHAPE.test(text) && takeConnectionString(text);
  }

  async function test() {
    setTesting(true);
    setTestResult(null);
    try {
      const profile = snapshot();
      const info = await withConnectPrompts(
        { connectionId: profile.id || null, connectionName: profile.name },
        secrets => safeInvoke("test_connection", { profile, secrets }),
        typedSecrets(),
      );
      if (info) setTestResult({ ok: true, message: `Connected to ${info.product} ${info.version}` });
    } catch (error) {
      setTestResult({ ok: false, message: errorMessage(error) });
    } finally {
      setTesting(false);
    }
  }

  async function save() {
    setSaving(true);
    try {
      const saved = await connections.save(snapshot());

      for (const [slot, value] of Object.entries(pending)) {
        if (value === null) {
          await safeInvoke("clear_connection_secret", { connectionId: saved.id, slot });
        } else if (typeof value === "string") {
          await safeInvoke("set_connection_secret", { connectionId: saved.id, slot, value });
        }
      }

      pushToast(`Saved ${saved.name}`, "success", 2500);
      props.onClose();
    } catch (error) {
      pushError(error);
    } finally {
      setSaving(false);
    }
  }

  return (
    <Modal
      show={props.show}
      onHide={props.onClose}
      title={isNew() ? "New connection" : "Edit connection"}
      subtitle={target()}
      width={620}
      footer={
        <>
          <div class={styles.TestArea}>
            <Button variant="subtle" onClick={test} disabled={testing()}>
              {testing() ? <Spinner size={14} /> : <Plug size={14} />}
              Test connection
            </Button>
            <Show when={testResult()}>
              {result => (
                <span
                  ref={element => tooltip(element, () => result().message)}
                  class={styles.TestResult}
                  classList={{ [styles.Failed]: !result().ok }}
                >
                  {result().ok ? <CircleCheck size={14} /> : <CircleAlert size={14} />}
                  <span>{result().message}</span>
                </span>
              )}
            </Show>
          </div>
          <Button variant="subtle" onClick={props.onClose}>Cancel</Button>
          <Button variant="primary" onClick={save} disabled={saving()}>
            {saving() ? "Saving" : "Save"}
          </Button>
        </>
      }
    >
      <div class={styles.Editor}>
        <FieldRow>
          <Field label="Name" grow>
            <Input
              value={draft.name}
              onValue={value => setDraft("name", value)}
              placeholder={suggestedName()}
              autofocus
            />
          </Field>
          <Field label="Type" width={150}>
            <Select
              value={draft.kind}
              items={databaseModules.map(module => ({ value: module.kind, label: module.label }))}
              onChange={changeKind}
              disabled={databaseModules.length < 2}
            />
          </Field>
        </FieldRow>

        <FieldRow>
          <Field label="Group" grow>
            <SuggestInput
              class={styles.GroupInput}
              value={draft.group ?? ""}
              suggestions={groups()}
              onCommit={value => setDraft("group", value || null)}
              placeholder="No group"
            />
          </Field>
          <Field label="Color">
            <div class={styles.Colors}>
              <button
                class={styles.Swatch}
                classList={{ [styles.Selected]: draft.color === null }}
                aria-label="Default color"
                onClick={() => setDraft("color", null)}
              />
              <For each={COLORS}>
                {color => (
                  <button
                    class={styles.Swatch}
                    classList={{ [styles.Selected]: draft.color === color }}
                    style={{ "--swatch": color }}
                    aria-label={color}
                    onClick={() => setDraft("color", color)}
                  />
                )}
              </For>
              <button
                ref={element => tooltip(element, () => "Custom color")}
                class={`${styles.Swatch} ${styles.Custom}`}
                classList={{ [styles.Selected]: customColor() !== null }}
                style={{ "--swatch": customColor() ?? undefined }}
                aria-label="Custom color"
                aria-expanded={pickerAnchor() !== null}
                onClick={event => setPickerAnchor(pickerAnchor() ? null : event.currentTarget)}
              >
                <Show when={customColor() === null}>
                  <Pipette size={11} />
                </Show>
              </button>
              <Show when={pickerAnchor()}>
                {anchor => (
                  <ColorPicker
                    anchor={anchor()}
                    value={customColor() ?? draft.color ?? COLORS[0]}
                    onChange={hex => setDraft("color", hex)}
                    onClose={() => setPickerAnchor(null)}
                  />
                )}
              </Show>
            </div>
          </Field>
        </FieldRow>

        <Field label="Connection string">
          <div class={styles.ConnectionString}>
            <Input
              mono
              class={styles.ConnectionStringInput}
              value={connectionString()}
              onValue={value => {
                setConnectionString(value);
                setParseNote(null);
              }}
              onKeyDown={event => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  fillFromConnectionString();
                }
              }}
              placeholder="postgres://user:password@host:5432/database"
            />
            <Button variant="subtle" onClick={fillFromConnectionString} disabled={connectionString().trim() === ""}>
              <Wand2 size={14} />
              Fill in
            </Button>
          </div>
          <Show
            when={parseNote()}
            fallback={<span class={styles.ParseHint}>Optional. Fills the fields below from a URL like mysql://, postgres:// or jdbc:, a libpq key=value string or a SQLite file path.</span>}
          >
            {note => (
              <span class={styles.ParseNote} classList={{ [styles.Failed]: !note().ok }}>
                {note().ok ? <CircleCheck size={13} /> : <CircleAlert size={13} />}
                <span>{note().message}</span>
              </span>
            )}
          </Show>
        </Field>

        <div class={styles.Divider} />

        {withDatabaseModule(draft, (module, options) => (
          <module.ConnectionForm options={options} update={updateOptions} secrets={secretEditor} onConnectionString={pastedIntoHost} />
        ))}
      </div>
    </Modal>
  );
}

export default ConnectionEditor;
