import styles from "./SavedQueryEditor.module.scss";
import { Show, createEffect, createMemo, createSignal, on, untrack } from "solid-js";
import useWorkspaceState from "@core/states/workspaceState";
import useSavedQueriesState from "@core/states/savedQueriesState";
import useToastState from "@core/states/toastState";
import { databaseModule } from "@core/databases";
import { buildCatalog, buildCompletionSchema } from "@core/helpers/catalog";
import { SavedQuery } from "@core/types";
import Modal from "@core/components/Modal/Modal";
import Button from "@core/components/Button/Button";
import Field, { FieldRow } from "@core/components/Field/Field";
import Input from "@core/components/Input/Input";
import Select from "@core/components/Select/Select";
import SqlEditor, { SqlEditorApi } from "@core/components/SqlEditor/SqlEditor";

const NO_DATABASE = "";

export type SavedQueryEditorState = { query: SavedQuery | null } | null;

export type SavedQueryEditorProps = {
  sessionId: string,
  state: SavedQueryEditorState,
  onClose: () => void,
};

// Edits a saved query on its own. Tabs only ever get a copy of its SQL.
function SavedQueryEditor(props: SavedQueryEditorProps) {
  const workspace = useWorkspaceState;
  const savedQueries = useSavedQueriesState;
  const { pushToast, pushError } = useToastState;

  const [name, setName] = createSignal("");
  const [database, setDatabase] = createSignal(NO_DATABASE);
  const [saving, setSaving] = createSignal(false);
  let editor: SqlEditorApi | undefined;

  const session = createMemo(() => workspace.store.sessions[props.sessionId]);
  const kind = untrack(() => workspace.store.sessions[props.sessionId].info.kind);
  const isNew = createMemo(() => props.state?.query === null);

  createEffect(on(() => props.state, state => {
    if (!state) return;
    setName(state.query?.name ?? "");
    setDatabase(state.query?.database ?? session()?.selectedDatabase ?? NO_DATABASE);
    if (database()) workspace.ensureCatalog(props.sessionId, database());
  }));

  const databaseItems = createMemo(() => [
    { value: NO_DATABASE, label: "No database" },
    ...(session()?.databases.items ?? []).map(item => ({ value: item.name, label: item.name })),
  ]);

  const catalog = createMemo(() => {
    const current = session();
    return current && database() ? buildCatalog(current, database()) : null;
  });

  const completionSchema = createMemo(() => {
    const current = session();
    return current ? buildCompletionSchema(current) : undefined;
  });

  async function save() {
    const profile = session()?.profile;
    const trimmed = name().trim();
    if (!editor || !profile || !trimmed) return;

    setSaving(true);
    try {
      const existing = props.state?.query;
      await savedQueries.save({
        id: existing?.id ?? "",
        name: trimmed,
        sql: editor.text(),
        connectionId: existing?.connectionId ?? profile.id,
        database: database() || null,
        updatedAt: 0,
      });
      pushToast(`Saved ${trimmed}`, "success", 2000);
      props.onClose();
    } catch (error) {
      pushError(error);
    } finally {
      setSaving(false);
    }
  }

  return (
    <Modal
      show={props.state !== null}
      onHide={props.onClose}
      title={isNew() ? "New saved query" : "Edit saved query"}
      subtitle={isNew() ? undefined : props.state?.query?.name}
      width={760}
      footer={
        <>
          <Button variant="subtle" onClick={props.onClose}>Cancel</Button>
          <Button variant="primary" onClick={save} disabled={saving() || !name().trim()}>Save</Button>
        </>
      }
    >
      <div class={styles.Editor}>
        <FieldRow>
          <Field label="Name" grow>
            <Input value={name()} onValue={setName} autofocus={isNew()} />
          </Field>
          <Field label="Database" width={220}>
            <Select
              value={database()}
              items={databaseItems()}
              onChange={value => {
                setDatabase(value);
                if (value) workspace.ensureCatalog(props.sessionId, value);
              }}
            />
          </Field>
        </FieldRow>
        <Field label="SQL">
          <Show when={props.state} keyed>
            {state => (
              <div class={styles.Sql}>
                <SqlEditor
                  initialValue={state.query?.sql ?? ""}
                  dialect={databaseModule(kind).sqlDialect}
                  schema={completionSchema()}
                  defaultSchema={database() || undefined}
                  catalog={catalog()}
                  onSave={save}
                  onReady={api => {
                    editor = api;
                    if (!isNew()) api.focus();
                  }}
                />
              </div>
            )}
          </Show>
        </Field>
      </div>
    </Modal>
  );
}

export default SavedQueryEditor;
