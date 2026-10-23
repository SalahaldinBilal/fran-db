import { onCleanup } from "solid-js";
import { ClipboardPaste, Copy, Scissors, TextSelect } from "lucide-solid";
import useContextMenuState from "@core/states/contextMenuState";
import { pasteText } from "./clipboard";

function editableTarget(target: EventTarget | null): HTMLElement | null {
  if (!(target instanceof HTMLElement)) return null;
  if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement) return target;
  const editor = target.closest<HTMLElement>(".cm-content");
  if (editor) return editor;
  const editable = target.closest<HTMLElement>("[contenteditable]");
  return editable?.isContentEditable ? editable : null;
}

function isWritable(element: HTMLElement): boolean {
  if (element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement) {
    return !element.readOnly && !element.disabled;
  }
  return element.isContentEditable;
}

function hasSelection(element: HTMLElement): boolean {
  if (element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement) {
    return element.selectionStart !== element.selectionEnd;
  }
  return (window.getSelection()?.toString() ?? "") !== "";
}

// The webview's own menu (reload, inspect) never shows. Text fields get a plain edit menu instead.
export function installGlobalContextMenu() {
  const { openContextMenu } = useContextMenuState;

  function onContextMenu(event: MouseEvent) {
    if (event.defaultPrevented) return;
    event.preventDefault();

    const target = editableTarget(event.target);
    if (!target) return;

    const writable = isWritable(target);
    const selected = hasSelection(target);

    openContextMenu(event, [
      { label: "Cut", icon: Scissors, hint: "Ctrl+X", disabled: !writable || !selected, onSelect: () => document.execCommand("cut") },
      { label: "Copy", icon: Copy, hint: "Ctrl+C", disabled: !selected, onSelect: () => document.execCommand("copy") },
      {
        label: "Paste",
        icon: ClipboardPaste,
        hint: "Ctrl+V",
        disabled: !writable,
        onSelect: async () => {
          const text = await pasteText();
          if (text) {
            target.focus();
            document.execCommand("insertText", false, text);
          }
        },
      },
      { separator: true },
      { label: "Select all", icon: TextSelect, hint: "Ctrl+A", onSelect: () => {
        target.focus();
        document.execCommand("selectAll");
      } },
    ]);
  }

  document.addEventListener("contextmenu", onContextMenu);
  onCleanup(() => document.removeEventListener("contextmenu", onContextMenu));
}
