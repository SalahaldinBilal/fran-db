import styles from "./Button.module.scss";
import { JSX, splitProps } from "solid-js";
import { tooltip } from "../Tooltip/Tooltip";

export type ButtonVariant = "ghost" | "subtle" | "primary" | "danger";

export type ButtonProps = {
  variant?: ButtonVariant,
  size?: "sm" | "md",
  icon?: boolean,
  active?: boolean,
  tooltip?: string,
  type?: "button" | "submit",
  disabled?: boolean,
  class?: string,
  style?: JSX.CSSProperties,
  children?: JSX.Element,
  onClick?: JSX.EventHandler<HTMLButtonElement, MouseEvent>,
  ref?: (element: HTMLButtonElement) => void,
};

function Button(props: ButtonProps) {
  const [local] = splitProps(props, ["variant", "size", "icon", "active", "tooltip"]);

  return (
    <button
      ref={button => {
        tooltip(button, () => local.tooltip);
        props.ref?.(button);
      }}
      type={props.type ?? "button"}
      class={`${styles.Button} ${styles[local.variant ?? "ghost"]} ${props.class ?? ""}`}
      classList={{
        [styles.Small]: local.size === "sm",
        [styles.Icon]: local.icon ?? false,
        [styles.Active]: local.active ?? false,
      }}
      style={props.style}
      disabled={props.disabled}
      aria-label={local.icon ? local.tooltip : undefined}
      onClick={event => props.onClick?.(event)}
    >
      {props.children}
    </button>
  );
}

export default Button;
