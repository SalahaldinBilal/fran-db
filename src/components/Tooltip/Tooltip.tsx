import { Accessor, createEffect, createMemo, createSignal, on, onCleanup } from "solid-js";
import useTooltipState, { TooltipAnchor, TooltipOptions, TooltipValue } from "@core/states/tooltipState";

function toOptions(value: TooltipValue): TooltipOptions | null {
  if (value === null || value === undefined || value === "") return null;
  return typeof value === "string" ? { content: value } : value;
}

// Called from a ref, so its cleanup runs with the component that owns the element.
export function tooltip(element: HTMLElement, value: Accessor<TooltipValue>) {
  const { active, showTooltip, hideTooltip, cancelPendingTooltip } = useTooltipState;
  const options = createMemo(() => toOptions(value()));
  const anchor: TooltipAnchor = { element, options };
  const [hovered, setHovered] = createSignal(false);
  const isActive = () => active()?.element === element;
  const hidden = createMemo(() => {
    const current = options();
    return current === null || current.blocked === true;
  });

  function onEnter() {
    setHovered(true);
    if (!isActive() && !hidden()) showTooltip(anchor);
  }

  function onLeave() {
    setHovered(false);
    if (isActive()) hideTooltip();
    else cancelPendingTooltip();
  }

  createEffect(on(hidden, isHidden => {
    if (isHidden && isActive()) hideTooltip();
    else if (!isHidden && hovered() && !isActive()) showTooltip(anchor);
  }, { defer: true }));

  element.addEventListener("pointerenter", onEnter);
  element.addEventListener("pointerleave", onLeave);
  element.addEventListener("pointerdown", hideTooltip);

  onCleanup(() => {
    element.removeEventListener("pointerenter", onEnter);
    element.removeEventListener("pointerleave", onLeave);
    element.removeEventListener("pointerdown", hideTooltip);
    if (isActive()) hideTooltip();
    else if (hovered()) cancelPendingTooltip();
  });
}
