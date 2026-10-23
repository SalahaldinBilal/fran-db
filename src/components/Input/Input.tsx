import styles from "./Input.module.scss";
import { JSX, createSignal, onMount } from "solid-js";

export type InputProps = {
  value: string | number,
  onValue?: (value: string) => void,
  type?: "text" | "password" | "number" | "search",
  placeholder?: string,
  disabled?: boolean,
  readOnly?: boolean,
  mono?: boolean,
  autofocus?: boolean,
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

  onMount(() => {
    if (props.autofocus) input.focus();
  });

  return (
    <div
      class={`${styles.Input} ${props.class ?? ""}`}
      classList={{
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
        onKeyDown={event => props.onKeyDown?.(event)}
      />
      {props.after}
    </div>
  );
}

export default Input;
