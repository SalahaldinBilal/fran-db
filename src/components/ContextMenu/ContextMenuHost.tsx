import styles from "./ContextMenu.module.scss";
import { For, Show, createEffect, createSignal, onCleanup } from "solid-js";
import { Dynamic, Portal } from "solid-js/web";
import { ChevronRight } from "lucide-solid";
import useContextMenuState, { MenuAction, MenuItem, isSeparator } from "@core/states/contextMenuState";
import { Position } from "@core/types";
import { clickOutside } from "@core/directives/clickOutside";
clickOutside;

const EDGE_MARGIN = 4;

type MenuListProps = {
  items: MenuItem[],
  anchor: Position,
  // Where a submenu goes if it doesn't fit on the right: the parent item's left edge.
  flipX?: number,
};

function MenuList(props: MenuListProps) {
  const { closeContextMenu } = useContextMenuState;
  const [position, setPosition] = createSignal<Position>(props.anchor);
  const [openChild, setOpenChild] = createSignal<{ index: number, anchor: Position, flipX: number } | null>(null);
  let hoverTimer: ReturnType<typeof setTimeout> | undefined;

  onCleanup(() => clearTimeout(hoverTimer));

  function place(element: HTMLElement) {
    queueMicrotask(() => {
      const { offsetWidth: width, offsetHeight: height } = element;
      let x = props.anchor.x;
      if (x + width > window.innerWidth - EDGE_MARGIN) {
        x = props.flipX !== undefined ? props.flipX - width : window.innerWidth - width - EDGE_MARGIN;
      }
      const y = Math.min(props.anchor.y, window.innerHeight - height - EDGE_MARGIN);
      setPosition({ x: Math.max(EDGE_MARGIN, x), y: Math.max(EDGE_MARGIN, y) });
    });
  }

  function hover(index: number, item: MenuAction, element: HTMLElement) {
    clearTimeout(hoverTimer);
    hoverTimer = setTimeout(() => {
      if (!item.children || item.disabled) {
        setOpenChild(null);
        return;
      }
      const rect = element.getBoundingClientRect();
      setOpenChild({ index, anchor: { x: rect.right - 2, y: rect.top - 5 }, flipX: rect.left + 2 });
    }, item.children ? 80 : 150);
  }

  return (
    <div
      ref={place}
      class={styles.Menu}
      style={{ left: `${position().x}px`, top: `${position().y}px` }}
      role="menu"
    >
      <For each={props.items}>
        {(item, index) => (
          <Show when={!isSeparator(item) && (item as MenuAction)} fallback={<div class={styles.Separator} />}>
            {entry => (
              <>
                <button
                  class={styles.Item}
                  classList={{
                    [styles.Danger]: entry().danger ?? false,
                    [styles.Open]: openChild()?.index === index(),
                  }}
                  role="menuitem"
                  disabled={entry().disabled}
                  onMouseDown={event => event.preventDefault()}
                  onMouseEnter={event => hover(index(), entry(), event.currentTarget)}
                  onClick={event => {
                    if (entry().children) {
                      hover(index(), entry(), event.currentTarget);
                      return;
                    }
                    closeContextMenu();
                    entry().onSelect?.();
                  }}
                >
                  <span class={styles.Icon}>
                    <Show when={entry().icon}>
                      {icon => <Dynamic component={icon()} size={14} />}
                    </Show>
                  </span>
                  <span class={styles.Label}>{entry().label}</span>
                  <Show when={entry().hint}>
                    <span class={styles.Hint}>{entry().hint}</span>
                  </Show>
                  <Show when={entry().children}>
                    <ChevronRight size={14} class={styles.SubmenuArrow} />
                  </Show>
                </button>
                <Show when={openChild()?.index === index() && entry().children ? openChild() : null}>
                  {child => <MenuList items={entry().children!} anchor={child().anchor} flipX={child().flipX} />}
                </Show>
              </>
            )}
          </Show>
        )}
      </For>
    </div>
  );
}

function ContextMenuHost() {
  const { menu, closeContextMenu } = useContextMenuState;

  createEffect(() => {
    if (!menu()) return;

    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") closeContextMenu();
    }

    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("blur", closeContextMenu);
    window.addEventListener("resize", closeContextMenu);
    onCleanup(() => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("blur", closeContextMenu);
      window.removeEventListener("resize", closeContextMenu);
    });
  });

  return (
    <Show when={menu()} keyed>
      {current => (
        <Portal>
          <div
            class={styles.Root}
            use:clickOutside={closeContextMenu}
            onContextMenu={event => event.preventDefault()}
          >
            <MenuList items={current.items} anchor={current.position} />
          </div>
        </Portal>
      )}
    </Show>
  );
}

export default ContextMenuHost;
