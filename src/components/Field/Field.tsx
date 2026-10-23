import styles from "./Field.module.scss";
import { JSX, Show } from "solid-js";

export type FieldProps = {
  label: string,
  hint?: JSX.Element,
  grow?: boolean,
  width?: number,
  children: JSX.Element,
};

function Field(props: FieldProps) {
  return (
    <div
      class={styles.Field}
      classList={{ [styles.Grow]: props.grow ?? false }}
      style={{ width: props.width ? `${props.width}px` : undefined }}
    >
      <span class={styles.Label}>{props.label}</span>
      {props.children}
      <Show when={props.hint}>
        <span class={styles.Hint}>{props.hint}</span>
      </Show>
    </div>
  );
}

export function FieldRow(props: { children: JSX.Element }) {
  return <div class={styles.Row}>{props.children}</div>;
}

export function FieldSection(props: { title: string, description?: string, children: JSX.Element }) {
  return (
    <section class={styles.Section}>
      <header>
        <h3>{props.title}</h3>
        <Show when={props.description}><p>{props.description}</p></Show>
      </header>
      {props.children}
    </section>
  );
}

export default Field;
