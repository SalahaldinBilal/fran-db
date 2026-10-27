import styles from "./Settings.module.scss";
import { createMemo } from "solid-js";
import useSettingsState, {
  EXPRESSION_WIDTHS, FormatIndent, LetterCase, LINES_BETWEEN_QUERIES, OperatorPlacement,
} from "@core/states/settingsState";
import { formatStandardSql } from "@core/databases/shared/format";
import SegmentedControl from "@core/components/SegmentedControl/SegmentedControl";
import Select from "@core/components/Select/Select";
import { SettingGroup, SettingRow, SettingsPage } from "./SettingsLayout";

const SAMPLE_SQL = "select o.id, c.name as customer, count(i.id) as items, cast(sum(i.price * i.quantity) as decimal(10, 2)) as total "
  + "from orders o join customers c on c.id = o.customer_id left join order_items i on i.order_id = o.id "
  + "where o.status in ('paid', 'shipped') and (c.country = 'NL' or c.vip = true) "
  + "group by o.id, c.name having sum(i.price * i.quantity) > 100 order by total desc; "
  + "update customers set last_order_at = current_timestamp where id = 42;";

const CASE_OPTIONS: { value: LetterCase, label: string }[] = [
  { value: "upper", label: "UPPER" },
  { value: "lower", label: "lower" },
  { value: "preserve", label: "As written" },
];

const INDENT_OPTIONS: { value: FormatIndent, label: string }[] = [
  { value: "spaces2", label: "2 spaces" },
  { value: "spaces4", label: "4 spaces" },
  { value: "tabs", label: "Tabs" },
];

const OPERATOR_OPTIONS: { value: OperatorPlacement, label: string }[] = [
  { value: "after", label: "End of line" },
  { value: "before", label: "Start of line" },
];

const WIDTH_ITEMS = EXPRESSION_WIDTHS.map(width => ({ value: width, label: `${width} characters` }));
const BLANK_LINE_OPTIONS = LINES_BETWEEN_QUERIES.map(count => ({ value: count, label: count === 0 ? "None" : String(count) }));

function FormattingSettings() {
  const { settings, updateFormat } = useSettingsState;

  const preview = createMemo(() => {
    try {
      return formatStandardSql(SAMPLE_SQL, settings.format);
    } catch {
      return SAMPLE_SQL;
    }
  });

  return (
    <SettingsPage title="Formatting" description="How the Format button in a query tab lays out SQL.">
      <SettingGroup title="Letter case">
        <SettingRow name="Keywords" hint="SELECT, FROM, JOIN and the like.">
          <SegmentedControl<LetterCase>
            value={settings.format.keywordCase}
            options={CASE_OPTIONS}
            onChange={keywordCase => updateFormat({ keywordCase })}
          />
        </SettingRow>
        <SettingRow name="Functions" hint="Function names such as count and coalesce.">
          <SegmentedControl<LetterCase>
            value={settings.format.functionCase}
            options={CASE_OPTIONS}
            onChange={functionCase => updateFormat({ functionCase })}
          />
        </SettingRow>
        <SettingRow name="Data types" hint="Types in casts and definitions, such as varchar and decimal.">
          <SegmentedControl<LetterCase>
            value={settings.format.dataTypeCase}
            options={CASE_OPTIONS}
            onChange={dataTypeCase => updateFormat({ dataTypeCase })}
          />
        </SettingRow>
      </SettingGroup>

      <SettingGroup title="Layout">
        <SettingRow name="Indent">
          <SegmentedControl<FormatIndent>
            value={settings.format.indent}
            options={INDENT_OPTIONS}
            onChange={indent => updateFormat({ indent })}
          />
        </SettingRow>
        <SettingRow name="AND and OR" hint="Where they go when conditions are split over several lines.">
          <SegmentedControl<OperatorPlacement>
            value={settings.format.logicalOperatorNewline}
            options={OPERATOR_OPTIONS}
            onChange={logicalOperatorNewline => updateFormat({ logicalOperatorNewline })}
          />
        </SettingRow>
        <SettingRow name="Keep on one line up to" hint="Longer expressions in parentheses are split over several lines.">
          <Select
            size="sm"
            value={settings.format.expressionWidth}
            items={WIDTH_ITEMS}
            onChange={expressionWidth => updateFormat({ expressionWidth })}
          />
        </SettingRow>
        <SettingRow name="Blank lines between statements">
          <SegmentedControl<number>
            value={settings.format.linesBetweenQueries}
            options={BLANK_LINE_OPTIONS}
            onChange={linesBetweenQueries => updateFormat({ linesBetweenQueries })}
          />
        </SettingRow>
      </SettingGroup>

      <SettingGroup title="Preview">
        <pre class={styles.Preview}>{preview()}</pre>
      </SettingGroup>
    </SettingsPage>
  );
}

export default FormattingSettings;
