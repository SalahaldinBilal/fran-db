const EDGE_MARGIN = 8;

// Shifts a fixed-position popup sideways so it stays inside the window. `centered` is for popups
// placed with translateX(-50%), whose `left` is their center.
export function keepInViewport(element: HTMLElement, centered = false) {
  requestAnimationFrame(() => {
    const rect = element.getBoundingClientRect();
    const left = Math.min(Math.max(rect.left, EDGE_MARGIN), window.innerWidth - EDGE_MARGIN - rect.width);
    if (left === rect.left) return;
    element.style.left = `${centered ? left + rect.width / 2 : left}px`;
  });
}
