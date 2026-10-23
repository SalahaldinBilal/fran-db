import styles from "./Modal.module.scss";
import { JSX, Show, createEffect, createMemo, onCleanup } from "solid-js";
import { Portal } from "solid-js/web";
import { Transition } from "solid-transition-group";
import { makeEventListener } from "@solid-primitives/event-listener";
import { X } from "lucide-solid";
import Button from "../Button/Button";
import useModalState from "@core/states/modalState";

export type ModalProps = {
  show: boolean,
  onHide?: () => void,
  title?: JSX.Element,
  subtitle?: JSX.Element,
  footer?: JSX.Element,
  width?: number,
  dismissible?: boolean,
  children: JSX.Element,
};

function Modal(props: ModalProps) {
  const dismissible = createMemo(() => props.dismissible ?? true);

  createEffect(() => {
    if (props.show) onCleanup(useModalState.track());
  });

  makeEventListener(window, "keydown", event => {
    if (props.show && dismissible() && event.key === "Escape") props.onHide?.();
  });

  return (
    <Portal>
      <Transition
        onBeforeEnter={element => element.classList.add(styles.Entering)}
        onEnter={(element, done) => element.addEventListener("animationend", done, { once: true })}
        onAfterEnter={element => element.classList.remove(styles.Entering)}
        onBeforeExit={element => element.classList.add(styles.Exiting)}
        onExit={(element, done) => element.addEventListener("animationend", done, { once: true })}
      >
        <Show when={props.show}>
          <div
            class={styles.Backdrop}
            onMouseDown={event => {
              if (event.target === event.currentTarget && dismissible()) props.onHide?.();
            }}
          >
            <div
              class={styles.Dialog}
              role="dialog"
              aria-modal="true"
              style={{ width: props.width ? `${props.width}px` : undefined }}
            >
              <div class={styles.Header}>
                <div class={styles.Titles}>
                  <div class={styles.Title}>{props.title}</div>
                  <Show when={props.subtitle}>
                    <div class={styles.Subtitle}>{props.subtitle}</div>
                  </Show>
                </div>
                <Show when={dismissible()}>
                  <Button icon size="sm" tooltip="Close" onClick={() => props.onHide?.()}>
                    <X size={16} />
                  </Button>
                </Show>
              </div>
              <div class={styles.Content}>{props.children}</div>
              <Show when={props.footer}>
                <div class={styles.Footer}>{props.footer}</div>
              </Show>
            </div>
          </div>
        </Show>
      </Transition>
    </Portal>
  );
}

export default Modal;
