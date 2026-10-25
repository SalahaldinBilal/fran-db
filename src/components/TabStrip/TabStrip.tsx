import styles from "./TabStrip.module.scss";
import { For, JSX, Show, createEffect, createMemo, createSignal, on } from "solid-js";
import { Dynamic } from "solid-js/web";
import { LucideIcon, X } from "lucide-solid";

export type TabStripItem = {
  id: string,
  label: string,
  icon?: LucideIcon,
  dot?: string | null,
  tint?: string | null,
  closable?: boolean,
  renamable?: boolean,
  modified?: boolean,
  busy?: boolean,
};

export type TabStripApi = {
  startRename: (id: string) => void,
};

export type TabStripProps = {
  items: TabStripItem[],
  activeId: string | null,
  onSelect: (id: string) => void,
  onClose?: (id: string) => void,
  onRename?: (id: string, label: string) => void,
  onContextMenu?: (event: MouseEvent, id: string) => void,
  onReady?: (api: TabStripApi) => void,
  variant?: "global" | "inner",
  leading?: JSX.Element,
  trailing?: JSX.Element,
  /** Empty parts of the strip move the window, and some empty space is always kept before `trailing`. */
  dragRegion?: boolean,
  /** Tabs ignore the mouse, so pressing one drags the window instead. */
  locked?: boolean,
};

function TabStrip(props: TabStripProps) {
  const tabElements = new Map<string, HTMLElement>();
  const [renaming, setRenaming] = createSignal<string | null>(null);
  const dragAttribute = createMemo(() => (props.dragRegion ? "" : undefined));
  let scroller!: HTMLDivElement;

  createEffect(on(() => props.activeId, id => {
    if (id) tabElements.get(id)?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }));

  props.onReady?.({ startRename: id => setRenaming(id) });

  function commitRename(id: string, value: string) {
    setRenaming(null);
    const label = value.trim();
    if (label) props.onRename?.(id, label);
  }

  return (
    <div
      class={`${styles.Strip} ${styles[props.variant ?? "inner"]}`}
      classList={{ [styles.Locked]: props.locked ?? false }}
      data-tauri-drag-region={dragAttribute()}
    >
      <Show when={props.leading}>
        <div class={styles.Leading} data-tauri-drag-region={dragAttribute()}>{props.leading}</div>
      </Show>
      <div
        ref={scroller}
        class={styles.Tabs}
        role="tablist"
        data-tauri-drag-region={dragAttribute()}
        onWheel={event => {
          if (event.deltaY === 0) return;
          event.preventDefault();
          scroller.scrollLeft += event.deltaY;
        }}
      >
        <For each={props.items}>
          {item => (
            <div
              ref={element => tabElements.set(item.id, element)}
              role="tab"
              tabIndex={0}
              aria-selected={item.id === props.activeId}
              class={styles.Tab}
              classList={{ [styles.Active]: item.id === props.activeId, [styles.Tinted]: !!item.tint }}
              style={{ "--tint": item.tint ?? undefined }}
              onClick={() => props.onSelect(item.id)}
              onDblClick={() => {
                if (item.renamable) setRenaming(item.id);
              }}
              onKeyDown={event => {
                if (event.target !== event.currentTarget) return;
                if (event.key === "Enter" || event.key === " ") props.onSelect(item.id);
                if (event.key === "F2" && item.renamable) setRenaming(item.id);
              }}
              onMouseDown={event => {
                if (event.button === 1 && item.closable) event.preventDefault();
              }}
              onAuxClick={event => {
                if (event.button === 1 && item.closable) props.onClose?.(item.id);
              }}
              onContextMenu={event => props.onContextMenu?.(event, item.id)}
            >
              <Show when={item.dot}>
                {dot => <span class={styles.Dot} style={{ "background-color": dot() }} />}
              </Show>
              <Show when={item.icon}>
                {icon => (
                  <span class={styles.Icon} classList={{ [styles.Busy]: item.busy ?? false }}>
                    <Dynamic component={icon()} size={14} />
                  </span>
                )}
              </Show>
              <Show
                when={renaming() === item.id}
                fallback={<span class={styles.Label}>{item.label}</span>}
              >
                <input
                  class={styles.RenameInput}
                  value={item.label}
                  size={Math.max(6, item.label.length + 1)}
                  ref={input => queueMicrotask(() => {
                    input.focus();
                    input.select();
                  })}
                  onClick={event => event.stopPropagation()}
                  onDblClick={event => event.stopPropagation()}
                  onKeyDown={event => {
                    event.stopPropagation();
                    if (event.key === "Enter") commitRename(item.id, event.currentTarget.value);
                    if (event.key === "Escape") setRenaming(null);
                  }}
                  onBlur={event => {
                    if (renaming() === item.id) commitRename(item.id, event.currentTarget.value);
                  }}
                />
              </Show>
              <Show when={item.modified}>
                <span class={styles.Modified} aria-label="Unsaved changes" />
              </Show>
              <Show when={item.closable}>
                <button
                  class={styles.Close}
                  aria-label={`Close ${item.label}`}
                  onClick={event => {
                    event.stopPropagation();
                    props.onClose?.(item.id);
                  }}
                >
                  <X size={14} />
                </button>
              </Show>
            </div>
          )}
        </For>
      </div>
      <Show when={props.dragRegion}>
        <div class={styles.DragSpace} data-tauri-drag-region />
      </Show>
      <Show when={props.trailing}>
        <div class={styles.Trailing} data-tauri-drag-region={dragAttribute()}>{props.trailing}</div>
      </Show>
    </div>
  );
}

export default TabStrip;
