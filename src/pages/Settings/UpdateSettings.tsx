import styles from "./Settings.module.scss";
import { createMemo, Match, Show, Switch } from "solid-js";
import { Download, RefreshCw, RotateCw } from "lucide-solid";
import useUpdaterState from "@core/states/updaterState";
import Button from "@core/components/Button/Button";
import ReleaseNotes from "@core/components/ReleaseNotes/ReleaseNotes";
import { SettingGroup, SettingRow, SettingsPage } from "./SettingsLayout";

function UpdateSettings() {
  const updater = useUpdaterState;

  const appLabel = createMemo(() => (updater.currentVersion() ? `Fran DB v${updater.currentVersion()}` : "Fran DB"));

  const downloadPercent = createMemo(() => {
    const { downloaded, total } = updater.progress();
    return total ? Math.min(100, Math.round((downloaded / total) * 100)) : null;
  });

  const updateStatus = createMemo(() => {
    switch (updater.phase()) {
      case "idle": return "Not checked yet.";
      case "checking": return "Checking for updates…";
      case "upToDate": return "You're on the latest version.";
      case "available": return `Version ${updater.update()?.version} is available.${updater.selfUpdate ? "" : " Update it through Nix."}`;
      case "downloading": return downloadPercent() === null ? "Downloading the update…" : `Downloading the update, ${downloadPercent()}%`;
      case "ready": return "Restart Fran DB to finish updating.";
      case "error": return updater.error();
    }
  });

  const updateAction = createMemo(() => {
    const phase = updater.phase();
    if (phase === "ready") return "restart";
    if (!updater.selfUpdate) return "check";
    if (phase === "downloading" || (updater.update() && (phase === "available" || phase === "error"))) return "install";
    return "check";
  });

  const releaseNotes = createMemo(() => {
    const phase = updater.phase();
    return phase === "available" || phase === "downloading" ? updater.update()?.body?.trim() : undefined;
  });

  const progress = createMemo(() => (
    <Show when={updater.phase() === "downloading"}>
      <div class={styles.Progress}>
        <div
          class={styles.ProgressFill}
          classList={{ [styles.Indeterminate]: downloadPercent() === null }}
          style={{ width: `${downloadPercent() ?? 30}%` }}
        />
      </div>
    </Show>
  ));

  const notes = createMemo(() => (
    <Show when={releaseNotes()}>
      {text => <ReleaseNotes notes={text()} />}
    </Show>
  ));

  return (
    <SettingsPage title="Updates" description="The version you have and new releases.">
      <SettingGroup after={notes()}>
        <SettingRow name={appLabel()} hint={updateStatus()} hintError={updater.phase() === "error"} below={progress()}>
          <Switch>
            <Match when={updateAction() === "check"}>
              <Button variant="subtle" disabled={updater.phase() === "checking"} onClick={() => updater.checkForUpdate()}>
                <RefreshCw size={14} />
                Check now
              </Button>
            </Match>
            <Match when={updateAction() === "install"}>
              <Button variant="primary" disabled={updater.phase() === "downloading"} onClick={() => updater.install()}>
                <Download size={14} />
                Install update
              </Button>
            </Match>
            <Match when={updateAction() === "restart"}>
              <Button variant="primary" onClick={() => updater.restart()}>
                <RotateCw size={14} />
                Restart
              </Button>
            </Match>
          </Switch>
        </SettingRow>
      </SettingGroup>
    </SettingsPage>
  );
}

export default UpdateSettings;
