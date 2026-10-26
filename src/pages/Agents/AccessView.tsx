import styles from "./Agents.module.scss";
import { For, Show, createMemo, createSignal } from "solid-js";
import { Info, Plus, ShieldAlert, X } from "lucide-solid";
import { AgentAccess, AgentMode, ConnectionProfile } from "@core/types";
import { withDatabaseModule } from "@core/databases";
import useAgentState from "@core/states/agentState";
import useConnectionsState from "@core/states/connectionsState";
import useToastState from "@core/states/toastState";
import SegmentedControl from "@core/components/SegmentedControl/SegmentedControl";
import EmptyState from "@core/components/EmptyState/EmptyState";
import { tooltip } from "@core/components/Tooltip/Tooltip";

const MODES: { value: AgentMode, label: string }[] = [
  { value: "off", label: "Nothing" },
  { value: "readOnly", label: "Read" },
  { value: "ask", label: "Read + ask" },
];

function DatabaseChips(props: { access: AgentAccess, onChange: (databases: string[]) => void }) {
  const [adding, setAdding] = createSignal(false);
  const addLabel = createMemo(() => (props.access.databases.length === 0 ? "Limit" : "Add"));

  function commit(input: HTMLInputElement) {
    if (!adding()) return;
    const name = input.value.trim();
    setAdding(false);
    if (name && !props.access.databases.includes(name)) props.onChange([...props.access.databases, name]);
  }

  return (
    <div class={styles.Chips}>
      <Show when={props.access.databases.length === 0}>
        <span class={styles.AllNote}>All databases</span>
      </Show>
      <For each={props.access.databases}>
        {name => (
          <span class={styles.Chip}>
            {name}
            <button
              type="button"
              aria-label={`Stop sharing ${name}`}
              ref={element => tooltip(element, () => "Stop sharing")}
              onClick={() => props.onChange(props.access.databases.filter(existing => existing !== name))}
            >
              <X size={12} />
            </button>
          </span>
        )}
      </For>
      <Show
        when={adding()}
        fallback={
          <button type="button" class={`${styles.Chip} ${styles.AddChip}`} onClick={() => setAdding(true)}>
            <Plus size={12} />
            {addLabel()}
          </button>
        }
      >
        <input
          class={styles.ChipInput}
          placeholder="Database name"
          spellcheck={false}
          ref={input => queueMicrotask(() => input.focus())}
          onKeyDown={event => {
            if (event.key === "Enter") commit(event.currentTarget);
            if (event.key === "Escape") {
              event.stopPropagation();
              event.currentTarget.value = "";
              setAdding(false);
            }
          }}
          onBlur={event => commit(event.currentTarget)}
        />
      </Show>
    </div>
  );
}

function AccessRow(props: { profile: ConnectionProfile }) {
  const connections = useConnectionsState;
  const { pushError } = useToastState;

  const details = createMemo(() =>
    withDatabaseModule(props.profile, (module, options) => ({ kind: module.label, target: module.describeTarget(options) })),
  );
  const initial = createMemo(() => props.profile.name.trim().charAt(0).toUpperCase() || "?");

  async function save(access: AgentAccess) {
    try {
      await connections.save({ ...props.profile, agentAccess: access });
    } catch (error) {
      pushError(error);
    }
  }

  return (
    <div class={styles.TableRow}>
      <div class={styles.Connection}>
        <span class={styles.Badge} style={{ "--badge": props.profile.color ?? undefined }}>{initial()}</span>
        <div class={styles.ConnectionText}>
          <div class={styles.ConnectionName}>{props.profile.name}</div>
          <div class={styles.ConnectionHost} ref={element => tooltip(element, () => details().target)}>{details().target}</div>
        </div>
      </div>
      <span class={styles.Kind}>{details().kind}</span>
      <SegmentedControl<AgentMode>
        value={props.profile.agentAccess.mode}
        options={MODES}
        onChange={mode => save({ ...props.profile.agentAccess, mode })}
      />
      <Show when={props.profile.agentAccess.mode !== "off"} fallback={<span class={styles.Off}>Not shared</span>}>
        <DatabaseChips
          access={props.profile.agentAccess}
          onChange={databases => save({ ...props.profile.agentAccess, databases })}
        />
      </Show>
    </div>
  );
}

function AccessView() {
  const agent = useAgentState;
  const connections = useConnectionsState;

  return (
    <div class={styles.Scroll}>
      <div class={styles.Wrap}>
        <div class={styles.PageHead}>
          <h2>Access</h2>
          <p>What agents can do with each connection. New connections start at Nothing.</p>
        </div>

        <Show when={!agent.config.allowChanges}>
          <div class={styles.Notice}>
            <ShieldAlert size={14} />
            <span>Changes are turned off for every agent in Server, so Read + ask works like Read for now.</span>
          </div>
        </Show>

        <Show
          when={connections.connections.length > 0}
          fallback={<EmptyState title="No connections yet" description="Add a connection on the Connections tab, then choose what agents can do with it here." />}
        >
          <div class={styles.Table}>
            <div class={`${styles.TableRow} ${styles.TableHead}`}>
              <span>Connection</span>
              <span>Type</span>
              <span>Agents can</span>
              <span>Databases</span>
            </div>
            <For each={connections.connections}>
              {profile => <AccessRow profile={profile} />}
            </For>
            <div class={styles.TableFoot}>
              <Info size={14} />
              Read keeps agents to reads, but a database login that can only read is the only full guarantee. Changing a connection here closes its open agent sessions.
            </div>
          </div>
        </Show>
      </div>
    </div>
  );
}

export default AccessView;
