import styles from "./SettingsModal.module.scss";
import { createMemo, Match, Show, Switch } from "solid-js";
import { CircleAlert, Download, RefreshCw, RotateCw } from "lucide-solid";
import useSettingsState, { CellEditMode } from "@core/states/settingsState";
import useUpdaterState from "@core/states/updaterState";
import { errorMessage } from "@core/helpers/errors";
import Modal from "../Modal/Modal";
import Button from "../Button/Button";
import SegmentedControl from "../SegmentedControl/SegmentedControl";
import ReleaseNotes from "../ReleaseNotes/ReleaseNotes";

export type SettingsModalProps = {
  show: boolean,
  onClose: () => void,
};

const EDIT_MODE_HINTS: Record<CellEditMode, string> = {
  viewer: "Double-click or Enter opens the value viewer. Shift+Enter edits right in the cell.",
  inline: "Double-click or Enter edits right in the cell. Long text, JSON and binary values still open the viewer. Shift+Enter opens the viewer.",
};

function SettingsModal(props: SettingsModalProps) {
  const { settings, loadError, update } = useSettingsState;
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

  return (
    <Modal
      show={props.show}
      onHide={props.onClose}
      title="Settings"
      subtitle="Saved on this machine and applied right away."
      width={600}
      footer={<Button variant="primary" onClick={props.onClose}>Done</Button>}
    >
      <div class={styles.Body}>
        <Show when={loadError()}>
          <div class={styles.Error}>
            <CircleAlert size={14} />
            <span>Settings couldn't be loaded, so defaults are in use. {errorMessage(loadError())}</span>
          </div>
        </Show>

        <section class={styles.Section}>
          <h3>Results</h3>
          <div class={styles.Setting}>
            <div class={styles.Text}>
              <div class={styles.Name}>Edit cells in</div>
              <div class={styles.Hint}>{EDIT_MODE_HINTS[settings.cellEditMode]}</div>
            </div>
            <SegmentedControl<CellEditMode>
              value={settings.cellEditMode}
              onChange={mode => update({ cellEditMode: mode })}
              options={[
                { value: "viewer", label: "Viewer" },
                { value: "inline", label: "The cell" },
              ]}
            />
          </div>
        </section>

        <section class={styles.Section}>
          <h3>Updates</h3>
          <div class={styles.Setting}>
            <div class={styles.Text}>
              <div class={styles.Name}>{appLabel()}</div>
              <div class={styles.Hint} classList={{ [styles.HintError]: updater.phase() === "error" }}>{updateStatus()}</div>
              <Show when={updater.phase() === "downloading"}>
                <div class={styles.Progress}>
                  <div
                    class={styles.ProgressFill}
                    classList={{ [styles.Indeterminate]: downloadPercent() === null }}
                    style={{ width: `${downloadPercent() ?? 30}%` }}
                  />
                </div>
              </Show>
            </div>
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
          </div>

          <Show when={releaseNotes()}>
            {notes => <ReleaseNotes notes={notes()} />}
          </Show>
        </section>
      </div>
    </Modal>
  );
}

export default SettingsModal;
