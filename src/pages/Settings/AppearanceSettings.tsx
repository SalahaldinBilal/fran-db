import useSettingsState from "@core/states/settingsState";
import Toggle from "@core/components/Switch/Switch";
import { SettingGroup, SettingRow, SettingsPage } from "./SettingsLayout";

function AppearanceSettings() {
  const { settings, update } = useSettingsState;

  return (
    <SettingsPage title="Appearance" description="How Fran DB looks.">
      <SettingGroup>
        <SettingRow
          name="Fill with the connection color"
          hint="A connection with a color tints its whole tab and connection card. Turn this off to show the color only on the dot and icon."
        >
          <Toggle checked={settings.colorFill} onChange={checked => update({ colorFill: checked })} />
        </SettingRow>
      </SettingGroup>
    </SettingsPage>
  );
}

export default AppearanceSettings;
