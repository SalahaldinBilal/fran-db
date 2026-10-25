import shared from "../shared/ConnectionForm.module.scss";
import styles from "./SqliteConnectionForm.module.scss";
import { Match, Switch as SwitchCase, createSignal } from "solid-js";
import { SqliteOptions } from "@core/types";
import { ConnectionFormProps } from "../types";
import Field, { FieldSection } from "@core/components/Field/Field";
import Input from "@core/components/Input/Input";
import FilePathInput from "@core/components/FilePathInput/FilePathInput";
import SegmentedControl from "@core/components/SegmentedControl/SegmentedControl";
import Switch from "@core/components/Switch/Switch";
import { toTimeout } from "../shared/fields";

type Section = "connection" | "advanced";

function SqliteConnectionForm(props: ConnectionFormProps<SqliteOptions>) {
  const [section, setSection] = createSignal<Section>("connection");

  return (
    <div class={shared.Form}>
      <SegmentedControl
        fill
        value={section()}
        onChange={setSection}
        options={[
          { value: "connection", label: "Connection" },
          { value: "advanced", label: "Advanced" },
        ]}
      />

      <SwitchCase>
        <Match when={section() === "connection"}>
          <div class={shared.Sections}>
            <FieldSection title="SQLite">
              <Field label="Database file">
                <FilePathInput
                  value={props.options.path}
                  onValue={value => props.update(options => { options.path = value; })}
                  placeholder="/path/to/database.db"
                  dialogTitle="Choose a SQLite database"
                />
              </Field>
              <div class={styles.Toggles}>
                <Switch
                  checked={props.options.readOnly}
                  onChange={checked => props.update(options => { options.readOnly = checked; })}
                  label="Open read only"
                />
                <Switch
                  checked={props.options.createIfMissing}
                  disabled={props.options.readOnly}
                  onChange={checked => props.update(options => { options.createIfMissing = checked; })}
                  label="Create the file if it doesn't exist"
                />
              </div>
            </FieldSection>
          </div>
        </Match>

        <Match when={section() === "advanced"}>
          <div class={shared.Sections}>
            <Switch
              checked={props.options.foreignKeys}
              onChange={checked => props.update(options => { options.foreignKeys = checked; })}
              label="Enforce foreign keys"
            />
            <Field label="Busy timeout (seconds)" hint="How long to wait while another process holds a lock." width={260}>
              <Input
                type="number"
                min={1}
                value={props.options.busyTimeoutSecs}
                onValue={value => props.update(options => { options.busyTimeoutSecs = toTimeout(value); })}
              />
            </Field>
            <Field label="Run after connecting" hint="SQL run on every new connection, for example PRAGMA statements.">
              <textarea
                class={shared.Textarea}
                spellcheck={false}
                value={props.options.initSql}
                onInput={event => {
                  const value = event.currentTarget.value;
                  props.update(options => { options.initSql = value; });
                }}
                placeholder="PRAGMA journal_mode = WAL;"
              />
            </Field>
          </div>
        </Match>
      </SwitchCase>
    </div>
  );
}

export default SqliteConnectionForm;
