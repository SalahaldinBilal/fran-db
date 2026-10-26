import styles from "./TabStrip.module.scss";
import { For, JSX, Show, createEffect, createMemo, createSignal, on, onCleanup } from "solid-js";
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
  movable?: boolean,
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
  onReorder?: (id: string, toIndex: number) => void,
  onReady?: (api: TabStripApi) => void,
  variant?: "global" | "inner",
  leading?: JSX.Element,
  trailing?: JSX.Element,
  /** Empty parts of the strip move the window, and some empty space is always kept before `trailing`. */
  dragRegion?: boolean,
  /** Tabs ignore the mouse, so pressing one drags the window instead. */
  locked?: boolean,
};

const DRAG_THRESHOLD = 4;
const SCROLL_EDGE = 32;
const SCROLL_STEP = 8;

type TabDrag = {
  id: string,
  target: number,
  offsets: Record<string, number>,
};

type DragGesture = {
  id: string,
  pointerId: number,
  startX: number,
  x: number,
  started: boolean,
  from: number,
  min: number,
  max: number,
  startScroll: number,
  lefts: number[],
  widths: number[],
  gap: number,
};

function TabStrip(props: TabStripProps) {
  const tabElements = new Map<string, HTMLElement>();
  const [renaming, setRenaming] = createSignal<string | null>(null);
  const [drag, setDrag] = createSignal<TabDrag | null>(null);
  const dragAttribute = createMemo(() => (props.dragRegion ? "" : undefined));
  let scroller!: HTMLDivElement;
  let gesture: DragGesture | null = null;
  let scrollFrame = 0;

  createEffect(on(() => props.activeId, id => {
    if (id) tabElements.get(id)?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }));

  props.onReady?.({ startRename: id => setRenaming(id) });

  function commitRename(id: string, value: string) {
    setRenaming(null);
    const label = value.trim();
    if (label) props.onRename?.(id, label);
  }

  function movableRange(from: number): [number, number] {
    let min = from;
    let max = from;
    while (props.items[min - 1]?.movable) min--;
    while (props.items[max + 1]?.movable) max++;
    return [min, max];
  }

  function beginDrag(event: PointerEvent, item: TabStripItem) {
    if (event.button !== 0 || !item.movable || !props.onReorder || renaming() === item.id) return;
    if ((event.target as HTMLElement).closest("button, input")) return;

    const from = props.items.findIndex(candidate => candidate.id === item.id);
    const [min, max] = movableRange(from);
    if (min === max) return;

    gesture = {
      id: item.id,
      pointerId: event.pointerId,
      startX: event.clientX,
      x: event.clientX,
      started: false,
      from,
      min,
      max,
      startScroll: 0,
      lefts: [],
      widths: [],
      gap: 0,
    };
    window.addEventListener("pointermove", moveDrag);
    window.addEventListener("pointerup", endDrag);
    window.addEventListener("pointercancel", cancelDrag);
  }

  function startDrag(current: DragGesture) {
    props.onSelect(current.id);
    current.started = true;
    current.startScroll = scroller.scrollLeft;
    current.gap = parseFloat(getComputedStyle(scroller).columnGap) || 0;
    for (const item of props.items) {
      const rect = tabElements.get(item.id)?.getBoundingClientRect();
      current.lefts.push((rect?.left ?? 0) + scroller.scrollLeft);
      current.widths.push(rect?.width ?? 0);
    }
    tabElements.get(current.id)?.setPointerCapture(current.pointerId);
    scrollFrame = requestAnimationFrame(scrollWhileDragging);
  }

  function moveDrag(event: PointerEvent) {
    if (!gesture || event.pointerId !== gesture.pointerId) return;
    gesture.x = event.clientX;
    if (!gesture.started) {
      if (Math.abs(gesture.x - gesture.startX) < DRAG_THRESHOLD) return;
      startDrag(gesture);
    }
    updateDrag(gesture);
  }

  function updateDrag(current: DragGesture) {
    const { from, min, max, lefts, widths } = current;
    const width = widths[from];
    const moved = current.x - current.startX + scroller.scrollLeft - current.startScroll;
    const left = Math.min(Math.max(lefts[from] + moved, lefts[min]), lefts[max] + widths[max] - width);
    const center = left + width / 2;

    let target = from;
    for (let index = from + 1; index <= max; index++) {
      if (center > lefts[index] + widths[index] / 2) target = index;
    }
    for (let index = from - 1; index >= min; index--) {
      if (center < lefts[index] + widths[index] / 2) target = index;
    }

    const shift = width + current.gap;
    const offsets: Record<string, number> = { [current.id]: left - lefts[from] };
    for (let index = Math.min(from, target); index <= Math.max(from, target); index++) {
      if (index !== from) offsets[props.items[index].id] = index > from ? -shift : shift;
    }
    setDrag({ id: current.id, target, offsets });
  }

  function scrollWhileDragging() {
    if (!gesture?.started) return;
    const bounds = scroller.getBoundingClientRect();
    const step = gesture.x < bounds.left + SCROLL_EDGE ? -SCROLL_STEP
      : gesture.x > bounds.right - SCROLL_EDGE ? SCROLL_STEP
      : 0;
    if (step !== 0) {
      const before = scroller.scrollLeft;
      scroller.scrollLeft += step;
      if (scroller.scrollLeft !== before) updateDrag(gesture);
    }
    scrollFrame = requestAnimationFrame(scrollWhileDragging);
  }

  function finishDrag(commit: boolean) {
    window.removeEventListener("pointermove", moveDrag);
    window.removeEventListener("pointerup", endDrag);
    window.removeEventListener("pointercancel", cancelDrag);
    cancelAnimationFrame(scrollFrame);

    const current = gesture;
    const target = drag()?.target;
    gesture = null;
    setDrag(null);
    if (commit && current?.started && target !== undefined && target !== current.from) {
      props.onReorder?.(current.id, target);
    }
  }

  function endDrag(event: PointerEvent) {
    if (event.pointerId === gesture?.pointerId) finishDrag(true);
  }

  function cancelDrag(event: PointerEvent) {
    if (event.pointerId === gesture?.pointerId) finishDrag(false);
  }

  onCleanup(() => finishDrag(false));

  function tabTransform(id: string): string | undefined {
    const offset = drag()?.offsets[id];
    return offset ? `translateX(${offset}px)` : undefined;
  }

  return (
    <div
      class={`${styles.Strip} ${styles[props.variant ?? "inner"]}`}
      classList={{ [styles.Locked]: props.locked ?? false, [styles.Dragging]: drag() !== null }}
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
              classList={{
                [styles.Active]: item.id === props.activeId,
                [styles.Tinted]: !!item.tint,
                [styles.Lifted]: drag()?.id === item.id,
              }}
              style={{ "--tint": item.tint ?? undefined, transform: tabTransform(item.id) }}
              onPointerDown={event => beginDrag(event, item)}
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
