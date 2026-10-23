import styles from "./Select.module.scss";
import { For, JSX, Show, createEffect, createMemo, createSignal, onCleanup } from "solid-js";
import { Portal } from "solid-js/web";
import { keepInViewport } from "@core/helpers/viewport";
import { Check, ChevronDown } from "lucide-solid";

export type SelectItem<T> = {
  value: T,
  label: string,
  hint?: string,
  disabled?: boolean,
};

export type SelectProps<T> = {
  value: T,
  items: readonly SelectItem<T>[],
  onChange: (value: T) => void,
  placeholder?: string,
  disabled?: boolean,
  size?: "sm" | "md",
  style?: JSX.CSSProperties,
  class?: string,
};

const MENU_MARGIN = 4;
const MENU_MAX_HEIGHT = 300;

function Select<T>(props: SelectProps<T>) {
  const [open, setOpen] = createSignal(false);
  const [highlight, setHighlight] = createSignal(0);
  const [menuStyle, setMenuStyle] = createSignal<JSX.CSSProperties>({});
  const selected = createMemo(() => props.items.find(item => item.value === props.value));

  let control!: HTMLDivElement;
  let menu: HTMLDivElement | undefined;
  let followHighlight = false;

  function openMenu() {
    if (props.disabled) return;

    const rect = control.getBoundingClientRect();
    const spaceBelow = window.innerHeight - rect.bottom - MENU_MARGIN;
    const upwards = spaceBelow < Math.min(MENU_MAX_HEIGHT, 160) && rect.top > spaceBelow;

    setMenuStyle({
      left: `${rect.left}px`,
      "min-width": `${rect.width}px`,
      ...(upwards
        ? { bottom: `${window.innerHeight - rect.top + MENU_MARGIN}px` }
        : { top: `${rect.bottom + MENU_MARGIN}px` }),
    });
    followHighlight = true;
    const current = props.items.findIndex(item => item.value === props.value && !item.disabled);
    setHighlight(current >= 0 ? current : nextEnabled(-1, 1));
    setOpen(true);
  }

  function choose(item: SelectItem<T>) {
    if (item.disabled) return;
    props.onChange(item.value);
    setOpen(false);
  }

  // The nearest enabled item from `start` in `step` direction, or `start` when there is none.
  function nextEnabled(start: number, step: number): number {
    for (let index = start + step; index >= 0 && index < props.items.length; index += step) {
      if (!props.items[index].disabled) return index;
    }
    return start;
  }

  // The menu is positioned against the viewport, so any outside scroll or resize would detach it.
  createEffect(() => {
    if (!open()) return;

    function close(event: Event) {
      if (menu && event.target instanceof Node && menu.contains(event.target)) return;
      setOpen(false);
    }

    window.addEventListener("scroll", close, { capture: true });
    window.addEventListener("resize", close);
    onCleanup(() => {
      window.removeEventListener("scroll", close, { capture: true });
      window.removeEventListener("resize", close);
    });
  });

  createEffect(() => {
    const index = highlight();
    if (!open() || !menu || !followHighlight) return;

    followHighlight = false;
    const item = menu.children[index] as HTMLElement | undefined;
    if (!item) return;

    if (item.offsetTop < menu.scrollTop) {
      menu.scrollTop = item.offsetTop;
    } else if (item.offsetTop + item.offsetHeight > menu.scrollTop + menu.clientHeight) {
      menu.scrollTop = item.offsetTop + item.offsetHeight - menu.clientHeight;
    }
  });

  function onKeyDown(event: KeyboardEvent) {
    if (!open()) {
      if (["Enter", " ", "ArrowDown", "ArrowUp"].includes(event.key)) {
        event.preventDefault();
        openMenu();
      }
      return;
    }

    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        followHighlight = true;
        setHighlight(index => nextEnabled(index, 1));
        return;
      case "ArrowUp":
        event.preventDefault();
        followHighlight = true;
        setHighlight(index => nextEnabled(index, -1));
        return;
      case "Enter":
      case " ": {
        event.preventDefault();
        const item = props.items[highlight()];
        if (item) choose(item);
        return;
      }
      case "Escape":
        event.preventDefault();
        event.stopPropagation();
        setOpen(false);
        return;
      case "Tab":
        setOpen(false);
        return;
    }
  }

  return (
    <>
      <div
        ref={control}
        tabIndex={props.disabled ? -1 : 0}
        role="combobox"
        aria-expanded={open()}
        aria-haspopup="listbox"
        class={`${styles.Select} ${props.class ?? ""}`}
        classList={{
          [styles.Open]: open(),
          [styles.Small]: props.size === "sm",
          [styles.Disabled]: props.disabled ?? false,
        }}
        style={props.style}
        onClick={() => (open() ? setOpen(false) : openMenu())}
        onKeyDown={onKeyDown}
        onFocusOut={() => setOpen(false)}
      >
        <Show when={selected()} fallback={<span class={styles.Placeholder}>{props.placeholder ?? "Select"}</span>}>
          {item => <span class={styles.Label}>{item().label}</span>}
        </Show>
        <ChevronDown class={styles.Chevron} size={14} />
      </div>
      <Show when={open()}>
        <Portal>
          <div
            ref={element => {
              menu = element;
              keepInViewport(element);
            }}
            role="listbox"
            class={styles.Menu}
            style={menuStyle()}
            onMouseDown={event => event.preventDefault()}
          >
            <For each={props.items}>
              {(item, index) => (
                <div
                  role="option"
                  aria-selected={item.value === props.value}
                  aria-disabled={item.disabled ?? false}
                  class={styles.Item}
                  classList={{
                    [styles.Highlighted]: index() === highlight() && !item.disabled,
                    [styles.ItemDisabled]: item.disabled ?? false,
                  }}
                  onMouseEnter={() => {
                    if (!item.disabled) setHighlight(index());
                  }}
                  onClick={() => choose(item)}
                >
                  <span class={styles.Check}>
                    <Show when={item.value === props.value}><Check size={14} /></Show>
                  </span>
                  <span class={styles.ItemLabel}>{item.label}</span>
                  <Show when={item.hint}><span class={styles.Hint}>{item.hint}</span></Show>
                </div>
              )}
            </For>
          </div>
        </Portal>
      </Show>
    </>
  );
}

export default Select;
