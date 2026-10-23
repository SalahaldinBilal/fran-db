import { EditorView } from "@codemirror/view";

const LEAVING = "cm-tooltip-leaving";

// CodeMirror removes hover tooltips from the DOM at once. A short-lived copy fades out in their place.
export function fadeOutHoverTooltips(view: EditorView): () => void {
  const observer = new MutationObserver(records => {
    for (const record of records) {
      const parent = record.target;
      if (!(parent instanceof HTMLElement) || !parent.isConnected) continue;

      for (const node of record.removedNodes) {
        if (!(node instanceof HTMLElement)) continue;
        if (!node.classList.contains("cm-tooltip-hover") || node.classList.contains(LEAVING)) continue;

        const ghost = node.cloneNode(true) as HTMLElement;
        ghost.classList.add(LEAVING);
        ghost.addEventListener("animationend", () => ghost.remove(), { once: true });
        parent.appendChild(ghost);
      }
    }
  });

  observer.observe(view.dom, { childList: true, subtree: true });
  return () => observer.disconnect();
}
