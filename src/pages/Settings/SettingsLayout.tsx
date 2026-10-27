import styles from "./Settings.module.scss";
import { JSX, Show } from "solid-js";

export function SettingsPage(props: { title: string, description: string, children: JSX.Element }) {
  return (
    <>
      <div class={styles.PageHead}>
        <h2>{props.title}</h2>
        <p>{props.description}</p>
      </div>
      {props.children}
    </>
  );
}

export function SettingGroup(props: { title?: string, after?: JSX.Element, children: JSX.Element }) {
  return (
    <section class={styles.Section}>
      <Show when={props.title}>
        <h3>{props.title}</h3>
      </Show>
      <div class={styles.Group}>{props.children}</div>
      {props.after}
    </section>
  );
}

export type SettingRowProps = {
  name: JSX.Element,
  hint?: JSX.Element,
  hintError?: boolean,
  below?: JSX.Element,
  children?: JSX.Element,
};

export function SettingRow(props: SettingRowProps) {
  return (
    <div class={styles.Setting}>
      <div class={styles.Text}>
        <div class={styles.Name}>{props.name}</div>
        <Show when={props.hint}>
          <div class={styles.Hint} classList={{ [styles.HintError]: props.hintError ?? false }}>{props.hint}</div>
        </Show>
        {props.below}
      </div>
      {props.children}
    </div>
  );
}
