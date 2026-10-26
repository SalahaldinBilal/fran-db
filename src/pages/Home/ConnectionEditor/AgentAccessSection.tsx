import styles from "./ConnectionEditor.module.scss";
import { For, Show, createMemo, createSignal } from "solid-js";
import { Plus, RefreshCw, X } from "lucide-solid";
import { AgentAccess, AgentMode, DatabaseKind } from "@core/types";
import { databaseModule } from "@core/databases";
import { safeInvoke } from "@core/helpers/safeInvoke";
import { withConnectPrompts } from "@core/helpers/connectFlow";
import useAgentState from "@core/states/agentState";
import useToastState from "@core/states/toastState";
import Field, { FieldSection } from "@core/components/Field/Field";
import Input from "@core/components/Input/Input";
import Button from "@core/components/Button/Button";
import SegmentedControl from "@core/components/SegmentedControl/SegmentedControl";
import Spinner from "@core/components/Spinner/Spinner";

export type AgentAccessSectionProps = {
  access: AgentAccess,
  kind: DatabaseKind,
  // Empty while the connection isn't saved yet.
  connectionId: string,
  connectionName: string,
  onChange: (access: AgentAccess) => void,
};

const MODES: { value: AgentMode, label: string }[] = [
  { value: "off", label: "Nothing" },
  { value: "readOnly", label: "Read" },
  { value: "ask", label: "Read, and ask to change" },
];

function AgentAccessSection(props: AgentAccessSectionProps) {
  const agent = useAgentState;
  const { pushError } = useToastState;
  const [adding, setAdding] = createSignal("");
  const [serverNames, setServerNames] = createSignal<string[] | null>(null);
  const [loading, setLoading] = createSignal(false);

  const module = createMemo(() => databaseModule(props.kind));
  const listLabel = createMemo(() => (module().catalogLabel ? "Databases agents can open" : "Databases agents can use"));
  const offered = createMemo(() => (serverNames() ?? []).filter(name => !props.access.databases.includes(name)));
  const loadTooltip = createMemo(() => (props.connectionId ? "Connect and list the names on the server" : "Save the connection first"));
  const loadDisabled = createMemo(() => !props.connectionId || loading());

  const modeHint = createMemo(() => {
    switch (props.access.mode) {
      case "off":
        return "Agents don't see this connection.";
      case "readOnly":
        return "Agents can read data and schema. Only a login that can't write is a full guarantee.";
      case "ask":
        return agent.config.allowChanges
          ? "Agents can read, and every change waits for you to approve it here."
          : "Changes are turned off for all agents in Settings, so agents can only read for now.";
    }
  });

  function setDatabases(databases: string[]) {
    props.onChange({ ...props.access, databases });
  }

  function add(name: string) {
    const trimmed = name.trim();
    if (trimmed && !props.access.databases.includes(trimmed)) setDatabases([...props.access.databases, trimmed]);
    setAdding("");
  }

  async function loadNames() {
    if (!props.connectionId) return;
    setLoading(true);
    try {
      const session = await withConnectPrompts(
        { connectionId: props.connectionId, connectionName: props.connectionName },
        secrets => safeInvoke("open_session", { connectionId: props.connectionId, secrets }),
      );
      if (!session) return;
      try {
        const names = module().catalogLabel
          ? await safeInvoke("list_catalogs", { sessionId: session.id })
          : (await safeInvoke("list_databases", { sessionId: session.id }))
            .filter(database => !database.system)
            .map(database => database.name);
        setServerNames(names);
      } finally {
        await safeInvoke("close_session", { sessionId: session.id });
      }
    } catch (error) {
      pushError(error);
    } finally {
      setLoading(false);
    }
  }

  return (
    <FieldSection title="Agent access" description="What coding agents connected to Fran DB can do with this connection.">
      <Field label="Agents can" hint={modeHint()}>
        <SegmentedControl<AgentMode>
          value={props.access.mode}
          options={MODES}
          onChange={mode => props.onChange({ ...props.access, mode })}
        />
      </Field>

      <Show when={props.access.mode !== "off"}>
        <Field label={listLabel()} hint="Leave empty to share all of them.">
          <Show when={props.access.databases.length > 0}>
            <div class={styles.Names}>
              <For each={props.access.databases}>
                {name => (
                  <span class={styles.Name}>
                    {name}
                    <button
                      type="button"
                      aria-label={`Remove ${name}`}
                      onClick={() => setDatabases(props.access.databases.filter(existing => existing !== name))}
                    >
                      <X size={11} />
                    </button>
                  </span>
                )}
              </For>
            </div>
          </Show>
          <div class={styles.AddRow}>
            <Input
              value={adding()}
              onValue={setAdding}
              placeholder="Add a name"
              mono
              class={styles.AddInput}
              onKeyDown={event => {
                if (event.key === "Enter") add(adding());
              }}
            />
            <Button variant="subtle" disabled={!adding().trim()} onClick={() => add(adding())}>
              <Plus size={14} />
              Add
            </Button>
            <Button variant="subtle" tooltip={loadTooltip()} disabled={loadDisabled()} onClick={loadNames}>
              {loading() ? <Spinner size={14} /> : <RefreshCw size={14} />}
              Load names
            </Button>
          </div>
          <Show when={offered().length > 0}>
            <div class={styles.Names}>
              <For each={offered()}>
                {name => (
                  <button type="button" class={`${styles.Name} ${styles.Offered}`} onClick={() => add(name)}>
                    <Plus size={11} />
                    {name}
                  </button>
                )}
              </For>
            </div>
          </Show>
        </Field>
      </Show>
    </FieldSection>
  );
}

export default AgentAccessSection;
