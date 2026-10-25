import shared from "../shared/ConnectionForm.module.scss";
import { Match, Switch as SwitchCase, createSignal } from "solid-js";
import { D1Options } from "@core/types";
import { ConnectionFormProps } from "../types";
import Field, { FieldRow, FieldSection } from "@core/components/Field/Field";
import Input from "@core/components/Input/Input";
import SegmentedControl from "@core/components/SegmentedControl/SegmentedControl";
import SecretField from "@core/components/SecretField/SecretField";
import { toTimeout } from "../shared/fields";

type Section = "connection" | "advanced";

function D1ConnectionForm(props: ConnectionFormProps<D1Options>) {
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
            <FieldSection
              title="Cloudflare"
              description="Queries go through Cloudflare's HTTP API. The token needs the D1 Edit permission, or D1 Read to only browse."
            >
              <Field label="Account ID" hint="On the account's home page in the Cloudflare dashboard.">
                <Input
                  mono
                  value={props.options.accountId}
                  onValue={value => props.update(options => { options.accountId = value; })}
                />
              </Field>
              <Field label="API token">
                <SecretField slot="apiToken" secrets={props.secrets} />
              </Field>
            </FieldSection>
            <FieldSection title="D1">
              <FieldRow>
                <Field label="Database" hint="Its name or ID. Other databases in the account can be opened after connecting." grow>
                  <Input
                    value={props.options.database}
                    onValue={value => props.update(options => { options.database = value; })}
                    placeholder="my-database"
                  />
                </Field>
              </FieldRow>
            </FieldSection>
          </div>
        </Match>

        <Match when={section() === "advanced"}>
          <div class={shared.Sections}>
            <div class={shared.Note}>
              D1 runs each statement on its own, so BEGIN and COMMIT aren't available in query tabs. Saved edits still apply all
              together or not at all.
            </div>
            <Field label="Request timeout (seconds)" width={220}>
              <Input
                type="number"
                min={1}
                value={props.options.timeoutSecs}
                onValue={value => props.update(options => { options.timeoutSecs = toTimeout(value); })}
              />
            </Field>
          </div>
        </Match>
      </SwitchCase>
    </div>
  );
}

export default D1ConnectionForm;
