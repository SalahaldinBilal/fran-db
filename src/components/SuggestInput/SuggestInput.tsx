import menuStyles from "../Select/Select.module.scss";
import styles from "./SuggestInput.module.scss";
import { For, JSX, Show, createEffect, createMemo, createSignal, on, onCleanup } from "solid-js";
import { Portal } from "solid-js/web";
import { keepInViewport } from "@core/helpers/viewport";
import { Check } from "lucide-solid";

export type SuggestInputProps = {
  value: string,
  suggestions: readonly string[],
  // Called with the trimmed text when the user picks a suggestion, presses Enter or leaves the field.
  onCommit: (value: string) => void,
  class?: string,
  mono?: boolean,
  placeholder?: string,
  disabled?: boolean,
};

const MENU_MARGIN = 4;
const MENU_MAX_HEIGHT = 320;

// A free text input with a styled suggestion menu, in place of the browser's datalist popup.
function SuggestInput(props: SuggestInputProps) {
  const [text, setText] = createSignal(props.value);
  const [open, setOpen] = createSignal(false);
  const [query, setQuery] = createSignal("");
  const [highlight, setHighlight] = createSignal(-1);
  const [menuStyle, setMenuStyle] = createSignal<JSX.CSSProperties>({});

  let input!: HTMLInputElement;
  let menu: HTMLDivElement | undefined;
  let followHighlight = false;

  createEffect(on(() => props.value, value => {
    if (document.activeElement !== input) setText(value);
  }));

  const matches = createMemo(() => {
    const needle = query().trim().toLowerCase();
    if (!needle) return props.suggestions;
    return props.suggestions.filter(suggestion => suggestion.toLowerCase().includes(needle));
  });
  const visible = createMemo(() => open() && matches().length > 0);

  function place() {
    const rect = input.getBoundingClientRect();
    const spaceBelow = window.innerHeight - rect.bottom - MENU_MARGIN;
    const upwards = spaceBelow < Math.min(MENU_MAX_HEIGHT, 160) && rect.top > spaceBelow;
    setMenuStyle({
      left: `${rect.left}px`,
      "min-width": `${rect.width}px`,
      ...(upwards
        ? { bottom: `${window.innerHeight - rect.top + MENU_MARGIN}px` }
        : { top: `${rect.bottom + MENU_MARGIN}px` }),
    });
  }

  function openMenu(filter: string) {
    if (props.disabled) return;
    place();
    setQuery(filter);
    followHighlight = true;
    const current = text().trim().toLowerCase();
    setHighlight(matches().findIndex(suggestion => suggestion.toLowerCase() === current));
    setOpen(true);
  }

  function commit(value: string) {
    const trimmed = value.trim();
    setText(trimmed);
    if (trimmed !== props.value) props.onCommit(trimmed);
  }

  function pick(suggestion: string) {
    setOpen(false);
    commit(suggestion);
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
    if (!visible() || !menu || !followHighlight) return;

    followHighlight = false;
    const item = menu.children[index] as HTMLElement | undefined;
    if (!item) return;

    if (item.offsetTop < menu.scrollTop) {
      menu.scrollTop = item.offsetTop;
    } else if (item.offsetTop + item.offsetHeight > menu.scrollTop + menu.clientHeight) {
      menu.scrollTop = item.offsetTop + item.offsetHeight - menu.clientHeight;
    }
  });

  function moveHighlight(delta: number) {
    if (!open()) {
      openMenu("");
      return;
    }
    const count = matches().length;
    if (count === 0) return;
    followHighlight = true;
    setHighlight(index => (index < 0 ? (delta > 0 ? 0 : count - 1) : Math.min(Math.max(index + delta, 0), count - 1)));
  }

  function onKeyDown(event: KeyboardEvent) {
    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        moveHighlight(1);
        return;
      case "ArrowUp":
        event.preventDefault();
        moveHighlight(-1);
        return;
      case "Enter": {
        event.preventDefault();
        const suggestion = visible() ? matches()[highlight()] : undefined;
        if (suggestion !== undefined) pick(suggestion);
        else {
          setOpen(false);
          commit(text());
        }
        return;
      }
      case "Escape":
        if (!open()) return;
        event.preventDefault();
        event.stopPropagation();
        setOpen(false);
        return;
    }
  }

  return (
    <>
      <input
        ref={input}
        class={props.class}
        value={text()}
        placeholder={props.placeholder}
        disabled={props.disabled}
        spellcheck={false}
        autocomplete="off"
        role="combobox"
        aria-expanded={visible()}
        aria-autocomplete="list"
        onClick={() => {
          if (!open()) openMenu("");
        }}
        onInput={event => {
          const value = event.currentTarget.value;
          setText(value);
          if (open()) {
            setQuery(value);
            setHighlight(matches().length > 0 ? 0 : -1);
          } else {
            openMenu(value);
          }
        }}
        onKeyDown={onKeyDown}
        onBlur={() => {
          setOpen(false);
          commit(text());
        }}
      />
      <Show when={visible()}>
        <Portal>
          <div
            ref={element => {
              menu = element;
              keepInViewport(element);
            }}
            role="listbox"
            class={menuStyles.Menu}
            style={menuStyle()}
            onMouseDown={event => event.preventDefault()}
          >
            <For each={matches()}>
              {(suggestion, index) => (
                <div
                  role="option"
                  aria-selected={suggestion === props.value}
                  class={menuStyles.Item}
                  classList={{ [menuStyles.Highlighted]: index() === highlight() }}
                  onMouseEnter={() => setHighlight(index())}
                  onClick={() => pick(suggestion)}
                >
                  <span class={menuStyles.Check}>
                    <Show when={suggestion === props.value}><Check size={14} /></Show>
                  </span>
                  <span class={menuStyles.ItemLabel} classList={{ [styles.Mono]: props.mono ?? false }}>{suggestion}</span>
                </div>
              )}
            </For>
          </div>
        </Portal>
      </Show>
    </>
  );
}

export default SuggestInput;
