import styles from "./Button.module.scss";
import { JSX, Show, createSignal, onCleanup, splitProps } from "solid-js";
import { Portal } from "solid-js/web";
import { keepInViewport } from "@core/helpers/viewport";

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

const TOOLTIP_DELAY = 450;
const TOOLTIP_MARGIN = 8;

function Button(props: ButtonProps) {
  const [local] = splitProps(props, ["variant", "size", "icon", "active", "tooltip"]);
  const [tooltipStyle, setTooltipStyle] = createSignal<JSX.CSSProperties | null>(null);
  let element!: HTMLButtonElement;
  let timer: ReturnType<typeof setTimeout> | undefined;

  function scheduleTooltip() {
    if (!local.tooltip) return;
    timer = setTimeout(() => {
      const rect = element.getBoundingClientRect();
      const above = rect.bottom + 40 > window.innerHeight;
      setTooltipStyle({
        left: `${rect.left + rect.width / 2}px`,
        top: `${above ? rect.top - TOOLTIP_MARGIN : rect.bottom + TOOLTIP_MARGIN}px`,
        transform: `translate(-50%, ${above ? "-100%" : "0"})`,
      });
    }, TOOLTIP_DELAY);
  }

  function hideTooltip() {
    clearTimeout(timer);
    setTooltipStyle(null);
  }


  onCleanup(() => clearTimeout(timer));

  return (
    <button
      ref={button => {
        element = button;
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
      onClick={event => {
        hideTooltip();
        props.onClick?.(event);
      }}
      onMouseEnter={scheduleTooltip}
      onMouseLeave={hideTooltip}
    >
      {props.children}
      <Show when={tooltipStyle()}>
        {style => (
          <Portal>
            <div ref={element => keepInViewport(element, true)} class={styles.Tooltip} style={style()}>{local.tooltip}</div>
          </Portal>
        )}
      </Show>
    </button>
  );
}

export default Button;
