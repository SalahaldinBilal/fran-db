import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import useToastState from "@core/states/toastState";

export async function copyText(text: string, label?: string) {
  const { pushToast, pushError } = useToastState;
  try {
    await writeText(text);
    if (label) pushToast(`Copied ${label}`, "info", 1500);
  } catch (error) {
    pushError(error);
  }
}

export async function pasteText(): Promise<string | null> {
  try {
    return await readText();
  } catch {
    return null;
  }
}
