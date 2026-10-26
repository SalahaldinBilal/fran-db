import styles from "./SettingsModal.module.scss";
import { createMemo } from "solid-js";
import { Bot } from "lucide-solid";
import useAgentState from "@core/states/agentState";
import Button from "../Button/Button";
import Toggle from "../Switch/Switch";

export type AgentSettingsProps = {
  onOpenAgents: () => void,
};

function AgentSettings(props: AgentSettingsProps) {
  const agent = useAgentState;

  const hint = createMemo(() => {
    const status = agent.status();
    if (status.error) return status.error;
    if (status.running) return `Running at ${agent.url()}`;
    return "Lets coding agents use the connections you share with them.";
  });

  return (
    <section class={styles.Section}>
      <h3>Agents</h3>
      <div class={styles.Setting}>
        <div class={styles.Text}>
          <div class={styles.Name}>MCP server</div>
          <div class={styles.Hint} classList={{ [styles.HintError]: agent.status().error !== null }}>{hint()}</div>
        </div>
        <Button size="sm" variant="subtle" onClick={() => props.onOpenAgents()}>
          <Bot size={13} />
          Open Agents
        </Button>
        <Toggle checked={agent.config.enabled} onChange={checked => agent.update({ enabled: checked })} />
      </div>
    </section>
  );
}

export default AgentSettings;
