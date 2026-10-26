import styles from "./Input.module.scss";
import { JSX, Show, createMemo, createSignal, onMount } from "solid-js";
import { X } from "lucide-solid";
import { tooltip } from "../Tooltip/Tooltip";

export type InputProps = {
  value: string | number,
  onValue?: (value: string) => void,
  type?: "text" | "password" | "number" | "search",
  placeholder?: string,
  disabled?: boolean,
  readOnly?: boolean,
  mono?: boolean,
  autofocus?: boolean,
  clearable?: boolean,
  min?: number,
  max?: number,
  before?: JSX.Element,
  after?: JSX.Element,
  class?: string,
  style?: JSX.CSSProperties,
  onKeyDown?: JSX.EventHandler<HTMLInputElement, KeyboardEvent>,
  onBlur?: JSX.EventHandler<HTMLInputElement, FocusEvent>,
};

function Input(props: InputProps) {
  const [focused, setFocused] = createSignal(false);
  let input!: HTMLInputElement;

  const canClear = createMemo(() => (props.clearable ?? false) && String(props.value) !== "" && !props.disabled && !props.readOnly);

  onMount(() => {
    if (props.autofocus) input.focus();
  });

  function clear() {
    props.onValue?.("");
    input.focus();
  }

  function onKeyDown(event: KeyboardEvent & { currentTarget: HTMLInputElement, target: Element }) {
    if (event.key === "Escape" && canClear()) {
      event.preventDefault();
      event.stopPropagation();
      clear();
      return;
    }
    props.onKeyDown?.(event);
  }

  return (
    <div
      class={`${styles.Input} ${props.class ?? ""}`}
      classList={{
        [styles.Clearing]: canClear(),
        [styles.Focused]: focused(),
        [styles.Disabled]: props.disabled ?? false,
        [styles.Mono]: props.mono ?? false,
      }}
      style={props.style}
      onMouseDown={event => {
        if (event.target !== input) {
          event.preventDefault();
          input.focus();
        }
      }}
    >
      {props.before}
      <input
        ref={input}
        type={props.type ?? "text"}
        value={props.value}
        placeholder={props.placeholder}
        disabled={props.disabled}
        readOnly={props.readOnly}
        min={props.min}
        max={props.max}
        spellcheck={false}
        autocomplete="off"
        onInput={event => props.onValue?.(event.currentTarget.value)}
        onFocus={() => setFocused(true)}
        onBlur={event => {
          setFocused(false);
          props.onBlur?.(event);
        }}
        onKeyDown={onKeyDown}
      />
      <Show when={canClear()}>
        <button
          type="button"
          class={styles.Clear}
          tabIndex={-1}
          aria-label="Clear"
          ref={element => tooltip(element, () => "Clear")}
          onClick={clear}
        >
          <X size={12} />
        </button>
      </Show>
      {props.after}
    </div>
  );
}

export default Input;
