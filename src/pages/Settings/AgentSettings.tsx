import { createMemo } from "solid-js";
import { Bot } from "lucide-solid";
import useAgentState from "@core/states/agentState";
import Button from "@core/components/Button/Button";
import Toggle from "@core/components/Switch/Switch";
import { SettingGroup, SettingRow, SettingsPage } from "./SettingsLayout";

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
    <SettingsPage title="Agents" description="Access per connection, history and the server's port are on the Agents tab.">
      <SettingGroup>
        <SettingRow name="MCP server" hint={hint()} hintError={agent.status().error !== null}>
          <Button size="sm" variant="subtle" onClick={() => props.onOpenAgents()}>
            <Bot size={13} />
            Open Agents
          </Button>
          <Toggle checked={agent.config.enabled} onChange={checked => agent.update({ enabled: checked })} />
        </SettingRow>
      </SettingGroup>
    </SettingsPage>
  );
}

export default AgentSettings;
