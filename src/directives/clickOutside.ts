import { onCleanup } from "solid-js";

export function clickOutside(element: HTMLElement, accessor: () => (() => void)) {
  const onOutsideClick = accessor();

  function onPointerDown(event: MouseEvent) {
    if (event.target instanceof Node && !element.contains(event.target)) onOutsideClick();
  }

  document.addEventListener("mousedown", onPointerDown, { capture: true });
  onCleanup(() => document.removeEventListener("mousedown", onPointerDown, { capture: true }));
}
