import styles from "./ConnectionEditor.module.scss";
import { For, Show, createEffect, createMemo, createSignal, on } from "solid-js";
import { createStore, produce, reconcile, unwrap } from "solid-js/store";
import { CircleAlert, CircleCheck, Pipette, Plug } from "lucide-solid";
import { ConnectionProfile, DatabaseKind, DriverOptions } from "@core/types";
import { databaseModules, defaultDriver, withDatabaseModule } from "@core/databases";
import { PendingSecret, SecretEditor } from "@core/databases/types";
import { safeInvoke } from "@core/helpers/safeInvoke";
import { withConnectPrompts } from "@core/helpers/connectFlow";
import { errorMessage } from "@core/helpers/errors";
import useConnectionsState from "@core/states/connectionsState";
import useToastState from "@core/states/toastState";
import Modal from "@core/components/Modal/Modal";
import Button from "@core/components/Button/Button";
import Field, { FieldRow } from "@core/components/Field/Field";
import Input from "@core/components/Input/Input";
import Select from "@core/components/Select/Select";
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

function blankProfile(kind: DatabaseKind): ConnectionProfile {
  return { id: "", name: "", color: null, forwardPort: null, agentAccess: { mode: "off", databases: [] }, ...defaultDriver(kind) };
}

function ConnectionEditor(props: ConnectionEditorProps) {
  const connections = useConnectionsState;
  const { pushToast, pushError } = useToastState;

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

  createEffect(on(() => [props.show, props.profile] as const, ([show, profile]) => {
    if (!show) return;

    const initial: ConnectionProfile = profile ? JSON.parse(JSON.stringify(unwrap(profile))) : blankProfile("mysql");
    setDraft(reconcile(initial));
    setPending(reconcile({}));
    setStoredSlots([]);
    setTestResult(null);

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

        <div class={styles.Divider} />

        {withDatabaseModule(draft, (module, options) => (
          <module.ConnectionForm options={options} update={updateOptions} secrets={secretEditor} />
        ))}
      </div>
    </Modal>
  );
}

export default ConnectionEditor;
