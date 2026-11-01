export type Placement = { target: string, after: boolean } | { zone: string };

export type ReorderOptions = {
  id: string,
  items: () => HTMLElement[],
  zones?: () => HTMLElement[],
  axis: "x" | "y",
  ghost?: HTMLElement,
  onPlace: (placement: Placement) => void,
  onEnd: () => void,
};

const DRAG_THRESHOLD = 4;

function within(rect: DOMRect, x: number, y: number) {
  return x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom;
}

function swallowClick(event: Event) {
  event.stopPropagation();
  event.preventDefault();
}

export function startReorder(event: PointerEvent, options: ReorderOptions) {
  if (event.button !== 0) return;
  event.preventDefault();

  const pointerId = event.pointerId;
  const start = { x: event.clientX, y: event.clientY };
  let ghost: HTMLElement | null = null;
  let last: Placement | null = null;

  function dragged(): HTMLElement | undefined {
    return options.items().find(item => item.dataset.id === options.id);
  }

  function dim() {
    const element = dragged();
    if (element) element.style.opacity = ".35";
  }

  function lift() {
    const element = dragged();
    if (!element) return;

    const source = options.ghost ?? element;
    const rect = source.getBoundingClientRect();
    ghost = source.cloneNode(true) as HTMLElement;
    Object.assign(ghost.style, {
      position: "fixed",
      left: `${rect.left}px`,
      top: `${rect.top}px`,
      width: `${rect.width}px`,
      height: `${rect.height}px`,
      margin: "0",
      opacity: ".9",
      pointerEvents: "none",
      zIndex: "var(--z-modal)",
    });
    document.body.append(ghost);
    dim();

    document.body.style.cursor = "grabbing";
    document.body.style.userSelect = "none";
    document.body.style.pointerEvents = "none";
  }

  function place(placement: Placement) {
    if (JSON.stringify(placement) === JSON.stringify(last)) return;
    last = placement;
    options.onPlace(placement);
    dim();
  }

  function move(event: PointerEvent) {
    if (event.pointerId !== pointerId) return;

    const dx = event.clientX - start.x;
    const dy = event.clientY - start.y;

    if (!ghost) {
      if (Math.hypot(dx, dy) < DRAG_THRESHOLD) return;
      lift();
      if (!ghost) {
        end(event);
        return;
      }
    }

    ghost.style.transform = `translate(${dx}px, ${dy}px)`;

   const { clientX: x, clientY: y } = event;
    const list = options.items().filter(item => item.dataset.id !== options.id);

    const target = list.find(item => within(item.getBoundingClientRect(), x, y));
    if (target) {
      const rect = target.getBoundingClientRect();
      const after = options.axis === "x" ? x > rect.left + rect.width / 2 : y > rect.top + rect.height / 2;

      place({ target: target.dataset.id!, after });

      return;
    }

    const zone = options.zones?.().find(zone => within(zone.getBoundingClientRect(), x, y));
    if (zone && !list.some(item => zone.contains(item))) place({ zone: zone.dataset.id! });
  }

  function end(event: PointerEvent) {
    if (event.pointerId !== pointerId) return;

    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", end);
    window.removeEventListener("pointercancel", end);

    if (ghost) {
      ghost.remove();
      const element = dragged();
      if (element) element.style.opacity = "";

      document.body.style.cursor = "";
      document.body.style.userSelect = "";
      document.body.style.pointerEvents = "";

      window.addEventListener("click", swallowClick, true);
      setTimeout(() => window.removeEventListener("click", swallowClick, true));
    }
    options.onEnd();
  }

  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", end);
  window.addEventListener("pointercancel", end);
}
