import useSettingsState, { CellEditMode } from "@core/states/settingsState";
import SegmentedControl from "@core/components/SegmentedControl/SegmentedControl";
import { SettingGroup, SettingRow, SettingsPage } from "./SettingsLayout";

const EDIT_MODE_HINTS: Record<CellEditMode, string> = {
  viewer: "Double-click or Enter opens the value viewer. Shift+Enter edits right in the cell.",
  inline: "Double-click or Enter edits right in the cell. Long text, JSON and binary values still open the viewer. Shift+Enter opens the viewer.",
};

function ResultSettings() {
  const { settings, update } = useSettingsState;

  return (
    <SettingsPage title="Results" description="Working with the rows a query returns.">
      <SettingGroup>
        <SettingRow name="Edit cells in" hint={EDIT_MODE_HINTS[settings.cellEditMode]}>
          <SegmentedControl<CellEditMode>
            value={settings.cellEditMode}
            onChange={mode => update({ cellEditMode: mode })}
            options={[
              { value: "viewer", label: "Viewer" },
              { value: "inline", label: "The cell" },
            ]}
          />
        </SettingRow>
      </SettingGroup>
    </SettingsPage>
  );
}

export default ResultSettings;
