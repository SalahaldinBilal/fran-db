import styles from "./Agents.module.scss";
import { Show, createEffect, createMemo, createResource, createSignal } from "solid-js";
import { Braces, Trash2 } from "lucide-solid";
import useAgentState from "@core/states/agentState";
import usePromptState from "@core/states/promptState";
import useToastState from "@core/states/toastState";
import { copyText } from "@core/helpers/clipboard";
import { safeInvoke } from "@core/helpers/safeInvoke";
import { formatBytes } from "@core/helpers";
import Button from "@core/components/Button/Button";
import Input from "@core/components/Input/Input";
import Toggle from "@core/components/Switch/Switch";

function NumberSetting(props: { value: number, min: number, max: number, suffix?: string, onCommit: (value: number) => void }) {
  const [text, setText] = createSignal(String(props.value));
  const suffix = createMemo(() => (props.suffix ? <span class={styles.Suffix}>{props.suffix}</span> : undefined));
  createEffect(() => setText(String(props.value)));

  function commit() {
    const value = Number(text());
    if (Number.isInteger(value) && value >= props.min && value <= props.max) {
      if (value !== props.value) props.onCommit(value);
    } else {
      setText(String(props.value));
    }
  }

  return (
    <Input
      type="number"
      class={styles.NumberInput}
      value={text()}
      min={props.min}
      max={props.max}
      onValue={setText}
      onBlur={commit}
      onKeyDown={event => {
        if (event.key === "Enter") event.currentTarget.blur();
      }}
      after={suffix()}
    />
  );
}

function ServerView() {
  const agent = useAgentState;
  const { confirm } = usePromptState;
  const { pushToast, pushError } = useToastState;
  const [size, { refetch }] = createResource(() => safeInvoke("get_agent_history_size"));

  const jsonConfig = createMemo(() => JSON.stringify({ mcpServers: { "fran-db": { type: "http", url: agent.url() } } }, null, 2));
  const keptHint = createMemo(() => {
    const bytes = size();
    return bytes === undefined ? "Older days are deleted." : `Older days are deleted. ${formatBytes(bytes)} kept right now.`;
  });

  async function clearHistory() {
    const confirmed = await confirm({
      title: "Clear agent history",
      message: "Every recorded agent call is deleted. This can't be undone.",
      confirmLabel: "Clear history",
      danger: true,
    });
    if (!confirmed) return;
    try {
      await safeInvoke("clear_agent_history");
      pushToast("Agent history cleared", "success", 2500);
    } catch (error) {
      pushError(error);
    }
    refetch();
  }

  return (
    <div class={styles.Scroll}>
      <div class={`${styles.Wrap} ${styles.Narrow}`}>
        <div class={styles.PageHead}>
          <h2>Server</h2>
          <p>How agents reach Fran DB, what every agent may do, and what's kept.</p>
        </div>

        <section class={styles.Section}>
          <h3>Connecting</h3>
          <div class={styles.Group}>
            <div class={styles.Setting}>
              <div class={styles.SettingText}>
                <div class={styles.SettingName}>Port</div>
                <div class={styles.SettingHint}>
                  Agent configs point at <code>{agent.address()}</code>, so changing the port means updating them.
                </div>
                <Show when={agent.status().error}>
                  {error => <div class={`${styles.SettingHint} ${styles.HintError}`}>{error()}</div>}
                </Show>
              </div>
              <NumberSetting value={agent.config.port} min={1} max={65535} onCommit={port => agent.update({ port })} />
            </div>
            <div class={styles.Setting}>
              <div class={styles.SettingText}>
                <div class={styles.SettingName}>Add to an agent</div>
                <div class={styles.SettingHint}>A JSON entry to paste into your agent's MCP settings.</div>
              </div>
              <Button variant="subtle" onClick={() => copyText(jsonConfig(), "the JSON")}>
                <Braces size={14} />
                Copy JSON
              </Button>
            </div>
          </div>
        </section>

        <section class={styles.Section}>
          <h3>Every agent</h3>
          <div class={styles.Group}>
            <div class={styles.Setting}>
              <div class={styles.SettingText}>
                <div class={styles.SettingName}>Allow raw SQL</div>
                <div class={styles.SettingHint}>
                  Agents can run SQL they write themselves, reads only unless the connection allows changes. Off leaves only the built-in tools.
                </div>
              </div>
              <Toggle checked={agent.config.allowRawSql} onChange={checked => agent.update({ allowRawSql: checked })} />
            </div>
            <div class={styles.Setting}>
              <div class={styles.SettingText}>
                <div class={styles.SettingName}>Allow changes</div>
                <div class={styles.SettingHint}>Off makes every connection read only for agents, whatever Access says.</div>
              </div>
              <Toggle checked={agent.config.allowChanges} onChange={checked => agent.update({ allowChanges: checked })} />
            </div>
          </div>
        </section>

        <section class={styles.Section}>
          <h3>History</h3>
          <div class={styles.Group}>
            <div class={styles.Setting}>
              <div class={styles.SettingText}>
                <div class={styles.SettingName}>Keep history</div>
                <div class={styles.SettingHint}>Every call, the SQL it ran and what the agent got back, encrypted on this machine.</div>
              </div>
              <Toggle checked={agent.config.historyEnabled} onChange={checked => agent.update({ historyEnabled: checked })} />
            </div>
            <div class={styles.Setting}>
              <div class={styles.SettingText}>
                <div class={styles.SettingName}>Keep for</div>
                <div class={styles.SettingHint}>{keptHint()}</div>
              </div>
              <NumberSetting
                value={agent.config.historyDays}
                min={1}
                max={3650}
                suffix="days"
                onCommit={historyDays => agent.update({ historyDays })}
              />
              <Button variant="danger" onClick={clearHistory}>
                <Trash2 size={14} />
                Clear
              </Button>
            </div>
          </div>
        </section>
      </div>
    </div>
  );
}

export default ServerView;
