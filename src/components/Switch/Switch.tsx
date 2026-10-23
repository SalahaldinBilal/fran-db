import styles from "./Switch.module.scss";

export type SwitchProps = {
  checked: boolean,
  onChange: (checked: boolean) => void,
  disabled?: boolean,
  label?: string,
};

function Switch(props: SwitchProps) {
  return (
    <label class={styles.Switch} classList={{ [styles.Disabled]: props.disabled ?? false }}>
      <input
        type="checkbox"
        checked={props.checked}
        disabled={props.disabled}
        onChange={event => props.onChange(event.currentTarget.checked)}
      />
      <span class={styles.Track} classList={{ [styles.Checked]: props.checked }}>
        <span class={styles.Thumb} />
      </span>
      {props.label}
    </label>
  );
}

export default Switch;
