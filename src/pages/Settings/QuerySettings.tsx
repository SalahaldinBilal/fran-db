import useSettingsState, { ROW_LIMITS } from "@core/states/settingsState";
import Toggle from "@core/components/Switch/Switch";
import Select from "@core/components/Select/Select";
import { SettingGroup, SettingRow, SettingsPage } from "./SettingsLayout";

function QuerySettings() {
  const { settings, update } = useSettingsState;

  return (
    <SettingsPage title="Queries" description="How query tabs start out.">
      <SettingGroup>
        <SettingRow
          name="Run the query when opening a table"
          hint="Double-clicking a table, or Query table in its menu, opens a SELECT for it in a query tab. With this on, the SELECT runs right away."
        >
          <Toggle checked={settings.runTableQueryOnOpen} onChange={checked => update({ runTableQueryOnOpen: checked })} />
        </SettingRow>
        <SettingRow
          name="Default row limit"
          hint="The limit new query tabs start with. Each tab can change its own. The SELECT made for a table includes it as LIMIT."
        >
          <Select
            size="sm"
            value={settings.defaultRowLimit}
            items={ROW_LIMITS}
            onChange={limit => update({ defaultRowLimit: limit })}
          />
        </SettingRow>
      </SettingGroup>
    </SettingsPage>
  );
}

export default QuerySettings;
