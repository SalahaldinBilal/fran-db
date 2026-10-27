import styles from "./Settings.module.scss";
import { createMemo, createSignal, For, JSX, Show } from "solid-js";
import { Bot, CircleAlert, Download, LucideIcon, Palette, SquareTerminal, Table2, WandSparkles } from "lucide-solid";
import useSettingsState from "@core/states/settingsState";
import useUpdaterState from "@core/states/updaterState";
import { errorMessage } from "@core/helpers/errors";
import AppearanceSettings from "./AppearanceSettings";
import QuerySettings from "./QuerySettings";
import FormattingSettings from "./FormattingSettings";
import ResultSettings from "./ResultSettings";
import AgentSettings from "./AgentSettings";
import UpdateSettings from "./UpdateSettings";

export type SettingsProps = {
  onOpenAgents: () => void,
};

type SettingsSection = {
  id: string,
  label: string,
  icon: LucideIcon,
  marked?: () => boolean,
  view: () => JSX.Element,
};

function Settings(props: SettingsProps) {
  const { loadError } = useSettingsState;
  const updater = useUpdaterState;

  const updatePending = createMemo(() => ["available", "downloading", "ready"].includes(updater.phase()));

  const sections: SettingsSection[] = [
    { id: "appearance", label: "Appearance", icon: Palette, view: () => <AppearanceSettings /> },
    { id: "queries", label: "Queries", icon: SquareTerminal, view: () => <QuerySettings /> },
    { id: "formatting", label: "Formatting", icon: WandSparkles, view: () => <FormattingSettings /> },
    { id: "results", label: "Results", icon: Table2, view: () => <ResultSettings /> },
    { id: "agents", label: "Agents", icon: Bot, view: () => <AgentSettings onOpenAgents={props.onOpenAgents} /> },
    { id: "updates", label: "Updates", icon: Download, marked: updatePending, view: () => <UpdateSettings /> },
  ];

  const [activeId, setActiveId] = createSignal(sections[0].id);

  return (
    <div class={styles.Settings}>
      <nav class={styles.Nav}>
        <For each={sections}>
          {section => (
            <button
              type="button"
              class={styles.NavItem}
              classList={{ [styles.Active]: activeId() === section.id }}
              onClick={() => setActiveId(section.id)}
            >
              <section.icon size={18} />
              {section.label}
              <Show when={section.marked?.()}>
                <span class={styles.Mark} />
              </Show>
            </button>
          )}
        </For>
        <div class={styles.NavNote}>Saved on this machine and applied right away.</div>
      </nav>

      <div class={styles.Content}>
        <div class={styles.Wrap}>
          <Show when={loadError()}>
            <div class={styles.Error}>
              <CircleAlert size={14} />
              <span>Settings couldn't be loaded, so defaults are in use. {errorMessage(loadError())}</span>
            </div>
          </Show>
          <For each={sections}>
            {section => <Show when={activeId() === section.id}>{section.view()}</Show>}
          </For>
        </div>
      </div>
    </div>
  );
}

export default Settings;
