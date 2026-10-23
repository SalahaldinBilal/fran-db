import styles from "./EmptyState.module.scss";
import { JSX, Show } from "solid-js";
import { Dynamic } from "solid-js/web";
import { LucideIcon } from "lucide-solid";

export type EmptyStateProps = {
  icon?: LucideIcon,
  title: string,
  description?: JSX.Element,
  actions?: JSX.Element,
  tone?: "default" | "danger",
};

function EmptyState(props: EmptyStateProps) {
  return (
    <div class={styles.EmptyState} classList={{ [styles.Danger]: props.tone === "danger" }}>
      <Show when={props.icon}>
        {icon => (
          <div class={styles.Icon}>
            <Dynamic component={icon()} size={22} />
          </div>
        )}
      </Show>
      <div class={styles.Title}>{props.title}</div>
      <Show when={props.description}>
        <div class={styles.Description}>{props.description}</div>
      </Show>
      <Show when={props.actions}>
        <div class={styles.Actions}>{props.actions}</div>
      </Show>
    </div>
  );
}

export default EmptyState;
