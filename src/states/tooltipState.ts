import { Accessor, JSX, createRoot, createSignal } from "solid-js";

export type TooltipSide = "top" | "bottom" | "left" | "right";

export type TooltipOptions = {
  content: JSX.Element,
  side?: TooltipSide,
  blocked?: boolean,
};

// A plain string is shorthand for `{ content }`. Empty values show nothing.
export type TooltipValue = TooltipOptions | string | null | undefined;

export type TooltipAnchor = {
  element: HTMLElement,
  options: Accessor<TooltipOptions | null>,
};

const SHOW_DELAY = 400;
const WARM_WINDOW = 300;

function useTooltipStateInner() {
  const [active, setActive] = createSignal<TooltipAnchor | null>(null);
  let showTimer: ReturnType<typeof setTimeout> | undefined;
  let lastHiddenAt = 0;

  // The first tooltip waits a moment. Moving to a neighbour while one is open, or just was, shows it right away.
  function showTooltip(anchor: TooltipAnchor) {
    clearTimeout(showTimer);
    const warm = active() !== null || performance.now() - lastHiddenAt < WARM_WINDOW;
    if (warm) {
      setActive(anchor);
      return;
    }
    showTimer = setTimeout(() => {
      if (anchor.element.isConnected) setActive(anchor);
    }, SHOW_DELAY);
  }

  function hideTooltip() {
    clearTimeout(showTimer);
    if (active()) lastHiddenAt = performance.now();
    setActive(null);
  }

  function cancelPendingTooltip() {
    clearTimeout(showTimer);
  }

  return { active, showTooltip, hideTooltip, cancelPendingTooltip };
}

const useTooltipState = createRoot(useTooltipStateInner);
export default useTooltipState;
