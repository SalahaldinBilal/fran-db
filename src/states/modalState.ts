import { createMemo, createRoot, createSignal } from "solid-js";

function useModalStateInner() {
  const [openCount, setOpenCount] = createSignal(0);
  const anyOpen = createMemo(() => openCount() > 0);

  /** Marks a modal as open until the returned function is called. */
  function track(): () => void {
    setOpenCount(count => count + 1);
    return () => setOpenCount(count => count - 1);
  }

  return { anyOpen, track };
}

const useModalState = createRoot(useModalStateInner);
export default useModalState;
