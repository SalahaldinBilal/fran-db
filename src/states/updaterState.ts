import { createRoot, createSignal } from "solid-js";
import { getVersion } from "@tauri-apps/api/app";
import { check, Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { errorMessage } from "@core/helpers/errors";

export type UpdatePhase = "idle" | "checking" | "upToDate" | "available" | "downloading" | "ready" | "error";

export type DownloadProgress = { downloaded: number, total: number | null };

function useUpdaterStateInner() {
  const selfUpdate = !import.meta.env.VITE_NIX_BUILD;
  const [phase, setPhase] = createSignal<UpdatePhase>("idle");
  const [update, setUpdate] = createSignal<Update | null>(null);
  const [progress, setProgress] = createSignal<DownloadProgress>({ downloaded: 0, total: null });
  const [error, setError] = createSignal("");
  const [currentVersion, setCurrentVersion] = createSignal("");

  getVersion().then(setCurrentVersion, () => {});

  async function checkForUpdate(): Promise<Update | null> {
    if (phase() === "checking" || phase() === "downloading") return update();

    setPhase("checking");
    setError("");
    try {
      const found = await check();
      setUpdate(found);
      setPhase(found ? "available" : "upToDate");
      return found;
    } catch (caught) {
      setError(`Couldn't check for updates. ${errorMessage(caught)}`);
      setPhase("error");
      return null;
    }
  }

  async function install() {
    const pending = update();
    if (!pending || phase() === "downloading") return;

    setPhase("downloading");
    setError("");
    setProgress({ downloaded: 0, total: null });
    try {
      await pending.downloadAndInstall(event => {
        if (event.event === "Started") {
          setProgress({ downloaded: 0, total: event.data.contentLength ?? null });
        } else if (event.event === "Progress") {
          setProgress(previous => ({ ...previous, downloaded: previous.downloaded + event.data.chunkLength }));
        }
      });
      setPhase("ready");
    } catch (caught) {
      setError(`The update couldn't be installed. ${errorMessage(caught)}`);
      setPhase("error");
    }
  }

  return { selfUpdate, phase, update, progress, error, currentVersion, checkForUpdate, install, restart: relaunch };
}

const useUpdaterState = createRoot(useUpdaterStateInner);
export default useUpdaterState;
