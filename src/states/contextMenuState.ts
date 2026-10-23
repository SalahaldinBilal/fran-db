import { createRoot, createSignal } from "solid-js";
import { LucideIcon } from "lucide-solid";
import { Position } from "@core/types";

export type MenuAction = {
  label: string,
  icon?: LucideIcon,
  hint?: string,
  danger?: boolean,
  disabled?: boolean,
  onSelect?: () => void,
  children?: MenuItem[],
};

export type MenuItem = MenuAction | { separator: true };

type OpenMenu = { position: Position, items: MenuItem[] };

export function isSeparator(item: MenuItem): item is { separator: true } {
  return "separator" in item;
}

function useContextMenuStateInner() {
  const [menu, setMenu] = createSignal<OpenMenu | null>(null);

  function openContextMenu(event: MouseEvent, items: MenuItem[]) {
    event.preventDefault();
    event.stopPropagation();
    setMenu({ position: { x: event.clientX, y: event.clientY }, items });
  }

  function openMenuAt(position: Position, items: MenuItem[]) {
    setMenu({ position, items });
  }

  function closeContextMenu() {
    setMenu(null);
  }

  return { menu, openContextMenu, openMenuAt, closeContextMenu };
}

const useContextMenuState = createRoot(useContextMenuStateInner);
export default useContextMenuState;
