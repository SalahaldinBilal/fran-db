import styles from "./Tooltip.module.scss";
import { JSX, createEffect, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import { Portal } from "solid-js/web";
import useTooltipState, { TooltipSide } from "@core/states/tooltipState";

type Placement = { left: number, top: number, side: TooltipSide, arrow: number };

const GAP = 8;
const EDGE_MARGIN = 8;
const ARROW_INSET = 10;

const OPPOSITE: Record<TooltipSide, TooltipSide> = { top: "bottom", bottom: "top", left: "right", right: "left" };
const SLIDE: Record<TooltipSide, [string, string]> = { top: ["0px", "4px"], bottom: ["0px", "-4px"], left: ["4px", "0px"], right: ["-4px", "0px"] };

const clamp = (value: number, min: number, max: number) => Math.min(Math.max(value, min), max);

// Set directly because placing measures the element, and a style binding would skip values it thinks haven't changed.
function moveTo(element: HTMLElement, left: number, top: number) {
  element.style.left = `${left}px`;
  element.style.top = `${top}px`;
}

function fits(side: TooltipSide, target: DOMRect, width: number, height: number): boolean {
  switch (side) {
    case "top": return target.top - GAP - height >= EDGE_MARGIN;
    case "bottom": return target.bottom + GAP + height <= window.innerHeight - EDGE_MARGIN;
    case "left": return target.left - GAP - width >= EDGE_MARGIN;
    case "right": return target.right + GAP + width <= window.innerWidth - EDGE_MARGIN;
  }
}

// Takes the preferred side unless only the opposite one fits, then keeps the tooltip in the window with the arrow on the target.
function place(anchor: HTMLElement, preferred: TooltipSide, tip: HTMLElement): Placement {
  moveTo(tip, 0, 0);
  const target = anchor.getBoundingClientRect();
  const width = tip.offsetWidth;
  const height = tip.offsetHeight;

  const side = fits(preferred, target, width, height) || !fits(OPPOSITE[preferred], target, width, height)
    ? preferred
    : OPPOSITE[preferred];
  const vertical = side === "top" || side === "bottom";

  let left: number;
  let top: number;
  if (vertical) {
    left = target.left + target.width / 2 - width / 2;
    top = side === "top" ? target.top - GAP - height : target.bottom + GAP;
  } else {
    left = side === "left" ? target.left - GAP - width : target.right + GAP;
    top = target.top + target.height / 2 - height / 2;
  }
  left = clamp(left, EDGE_MARGIN, window.innerWidth - EDGE_MARGIN - width);
  top = clamp(top, EDGE_MARGIN, window.innerHeight - EDGE_MARGIN - height);

  const arrow = vertical
    ? clamp(target.left + target.width / 2 - left, ARROW_INSET, width - ARROW_INSET)
    : clamp(target.top + target.height / 2 - top, ARROW_INSET, height - ARROW_INSET);

  return { left, top, side, arrow };
}

function TooltipHost() {
  const { active, hideTooltip } = useTooltipState;
  const [placement, setPlacement] = createSignal<Placement | null>(null);
  const [visible, setVisible] = createSignal(false);
  let tip!: HTMLDivElement;

  const shown = createMemo(() => {
    const anchor = active();
    const options = anchor?.options();
    return anchor && options && !options.blocked ? { element: anchor.element, options } : null;
  });
  const content = createMemo(() => shown()?.options.content);
  const side = createMemo(() => placement()?.side ?? "top");

  const arrowStyle = createMemo<JSX.CSSProperties>(() => {
    const current = placement();
    if (!current) return {};
    return current.side === "top" || current.side === "bottom"
      ? { left: `${current.arrow - 4}px` }
      : { top: `${current.arrow - 4}px` };
  });

  // Runs after the content is in the DOM, so it can be measured.
  createEffect(() => {
    const current = shown();
    if (!current) {
      setVisible(false);
      setPlacement(null);
      moveTo(tip, -9999, -9999);
      return;
    }
    content();
    const next = place(current.element, current.options.side ?? "top", tip);
    moveTo(tip, next.left, next.top);
    setPlacement(next);
    requestAnimationFrame(() => setVisible(shown() !== null));
  });

  onMount(() => {
    function onPointerDown(event: PointerEvent) {
      if (!active()?.element.contains(event.target as Node)) hideTooltip();
    }
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") hideTooltip();
    }
    function onScroll() {
      if (active()) hideTooltip();
    }

    document.addEventListener("pointerdown", onPointerDown, true);
    document.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("resize", hideTooltip);
    window.addEventListener("blur", hideTooltip);
    onCleanup(() => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      document.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("resize", hideTooltip);
      window.removeEventListener("blur", hideTooltip);
    });
  });

  return (
    <Portal>
      <div
        ref={tip}
        class={styles.Tooltip}
        classList={{
          [styles.Top]: side() === "top",
          [styles.Bottom]: side() === "bottom",
          [styles.Left]: side() === "left",
          [styles.Right]: side() === "right",
          [styles.Visible]: visible(),
        }}
        style={{ "--dx": SLIDE[side()][0], "--dy": SLIDE[side()][1] }}
        role="tooltip"
      >
        <span class={styles.Arrow} style={arrowStyle()} />
        <span class={styles.Content}>{content()}</span>
      </div>
    </Portal>
  );
}

export default TooltipHost;
