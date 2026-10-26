import styles from "./Agents.module.scss";
import { Show, createMemo } from "solid-js";
import { Activity, Copy, Server, Shield } from "lucide-solid";
import useAgentState, { AgentsView } from "@core/states/agentState";
import useConnectionsState from "@core/states/connectionsState";
import { copyText } from "@core/helpers/clipboard";
import TabStrip, { TabStripItem } from "@core/components/TabStrip/TabStrip";
import Toggle from "@core/components/Switch/Switch";
import { tooltip } from "@core/components/Tooltip/Tooltip";
import ActivityView from "./ActivityView";
import AccessView from "./AccessView";
import ServerView from "./ServerView";

function StatusChip() {
  const agent = useAgentState;
  const state = createMemo(() => {
    const status = agent.status();
    if (status.running) return "running";
    return status.error ? "error" : "off";
  });
  const label = createMemo(() => ({ running: "Running", error: "Couldn't start", off: "Off" })[state()]);

  return (
    <span
      class={styles.StatusChip}
      data-state={state()}
      ref={element => tooltip(element, () => agent.status().error ?? undefined)}
    >
      <span class={styles.Led} />
      {label()}
      <Show when={state() === "running"}>
        <code>{agent.address()}</code>
        <button
          type="button"
          class={styles.ChipButton}
          aria-label="Copy URL"
          ref={element => tooltip(element, () => "Copy URL")}
          onClick={() => copyText(agent.url(), "the URL")}
        >
          <Copy size={12} />
        </button>
      </Show>
    </span>
  );
}

function Agents() {
  const agent = useAgentState;
  const connections = useConnectionsState;

  const shared = createMemo(() => connections.connections.filter(profile => profile.agentAccess.mode !== "off").length);

  const tabs = createMemo<TabStripItem[]>(() => [
    {
      id: "activity",
      label: "Activity",
      icon: Activity,
      count: agent.approvals.length > 0 ? agent.approvals.length : undefined,
      countTone: "warning",
    },
    { id: "access", label: "Access", icon: Shield, count: shared() },
    { id: "server", label: "Server", icon: Server },
  ]);

  return (
    <div class={styles.Agents}>
      <TabStrip
        items={tabs()}
        activeId={agent.view()}
        onSelect={id => agent.setView(id as AgentsView)}
        trailingAtEnd
        trailing={
          <div class={styles.Status}>
            <StatusChip />
            <Toggle checked={agent.config.enabled} onChange={checked => agent.update({ enabled: checked })} />
          </div>
        }
      />
      <div class={styles.View} classList={{ [styles.Hidden]: agent.view() !== "activity" }}>
        <ActivityView />
      </div>
      <Show when={agent.view() === "access"}>
        <div class={styles.View}><AccessView /></div>
      </Show>
      <Show when={agent.view() === "server"}>
        <div class={styles.View}><ServerView /></div>
      </Show>
    </div>
  );
}

export default Agents;
