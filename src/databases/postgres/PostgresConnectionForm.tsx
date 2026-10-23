import shared from "../shared/ConnectionForm.module.scss";
import { Match, Show, Switch as SwitchCase, createMemo, createSignal } from "solid-js";
import { Cable, Network, Plug } from "lucide-solid";
import { PostgresConnectMethod, PostgresOptions, PostgresSslMode } from "@core/types";
import { ConnectionFormProps } from "../types";
import Field, { FieldRow, FieldSection } from "@core/components/Field/Field";
import Input from "@core/components/Input/Input";
import Select, { SelectItem } from "@core/components/Select/Select";
import SegmentedControl from "@core/components/SegmentedControl/SegmentedControl";
import SecretField from "@core/components/SecretField/SecretField";
import SshSection from "../shared/SshSection";
import SslFileFields from "../shared/SslFileFields";
import { IS_WINDOWS, toPort, toTimeout } from "../shared/fields";

type Section = "connection" | "ssl" | "advanced";

const ALL_METHODS = [
  { value: "tcp", label: "TCP/IP", icon: Network },
  { value: "socket", label: "Socket", icon: Plug },
  { value: "sshTunnel", label: "TCP/IP over SSH", icon: Cable },
] as const satisfies readonly { value: PostgresConnectMethod, label: string, icon: typeof Network }[];

const METHODS = ALL_METHODS.filter(method => !IS_WINDOWS || method.value !== "socket");

const SSL_MODES: SelectItem<PostgresSslMode>[] = [
  { value: "disable", label: "Off", hint: "Plain connection" },
  { value: "prefer", label: "If available", hint: "Encrypt when the server supports it" },
  { value: "require", label: "Required", hint: "Encrypt, check the certificate only with a CA file" },
  { value: "verifyCa", label: "Verify CA", hint: "Certificate must be signed by the CA" },
  { value: "verifyFull", label: "Verify full", hint: "CA plus host name match" },
];

function PostgresConnectionForm(props: ConnectionFormProps<PostgresOptions>) {
  const [section, setSection] = createSignal<Section>("connection");
  const isSsh = createMemo(() => props.options.method === "sshTunnel");
  const isSocket = createMemo(() => props.options.method === "socket");

  return (
    <div class={shared.Form}>
      <SegmentedControl
        fill
        value={section()}
        onChange={setSection}
        options={[
          { value: "connection", label: "Connection" },
          { value: "ssl", label: "SSL" },
          { value: "advanced", label: "Advanced" },
        ]}
      />

      <SwitchCase>
        <Match when={section() === "connection"}>
          <div class={shared.Sections}>
            <Field label="Connection method">
              <SegmentedControl
                fill
                value={props.options.method}
                options={METHODS}
                onChange={method => props.update(options => { options.method = method; })}
              />
            </Field>

            <Show when={isSsh()}>
              <SshSection
                ssh={props.options.ssh}
                description="The tunnel opens first, then Postgres is reached from the SSH server."
                secrets={props.secrets}
                update={mutate => props.update(options => mutate(options.ssh))}
              />
            </Show>

            <FieldSection title="PostgreSQL">
              <FieldRow>
                <Show
                  when={!isSocket()}
                  fallback={
                    <Field label="Socket directory" hint="Or the socket file itself." grow>
                      <Input
                        mono
                        value={props.options.socketPath}
                        onValue={value => props.update(options => { options.socketPath = value; })}
                        placeholder="/var/run/postgresql"
                      />
                    </Field>
                  }
                >
                  <Field label={isSsh() ? "Postgres host (from the SSH server)" : "Host"} grow>
                    <Input
                      value={props.options.host}
                      onValue={value => props.update(options => { options.host = value; })}
                      placeholder="127.0.0.1"
                    />
                  </Field>
                </Show>
                <Field label="Port" width={96}>
                  <Input
                    type="number"
                    value={props.options.port}
                    onValue={value => props.update(options => { options.port = toPort(value); })}
                  />
                </Field>
              </FieldRow>
              <FieldRow>
                <Field label="User" grow>
                  <Input
                    value={props.options.user}
                    onValue={value => props.update(options => { options.user = value; })}
                  />
                </Field>
                <Field label="Password" grow>
                  <SecretField slot="password" secrets={props.secrets} />
                </Field>
              </FieldRow>
              <FieldRow>
                <Field label="Database" hint="One connection works inside one database." grow>
                  <Input
                    value={props.options.database}
                    onValue={value => props.update(options => { options.database = value; })}
                    placeholder="postgres"
                  />
                </Field>
                <Field label="Default schema" hint="Opened after connecting." grow>
                  <Input
                    value={props.options.defaultSchema}
                    onValue={value => props.update(options => { options.defaultSchema = value; })}
                    placeholder="public"
                  />
                </Field>
              </FieldRow>
            </FieldSection>
          </div>
        </Match>

        <Match when={section() === "ssl"}>
          <div class={shared.Sections}>
            <Show when={isSocket()}>
              <div class={shared.Note}>SSL isn't used for socket connections.</div>
            </Show>
            <Field label="Use SSL">
              <Select
                value={props.options.ssl.mode}
                items={SSL_MODES}
                onChange={mode => props.update(options => { options.ssl.mode = mode; })}
              />
            </Field>
            <SslFileFields
              files={props.options.ssl}
              caHint="PEM file. Leave empty to trust the system's root certificates."
              update={mutate => props.update(options => mutate(options.ssl))}
            />
          </div>
        </Match>

        <Match when={section() === "advanced"}>
          <div class={shared.Sections}>
            <Field label="Connect timeout (seconds)" width={200}>
              <Input
                type="number"
                min={1}
                value={props.options.connectTimeoutSecs}
                onValue={value => props.update(options => { options.connectTimeoutSecs = toTimeout(value); })}
              />
            </Field>
            <Field label="Run after connecting" hint="SQL run on every new connection, for example SET statements.">
              <textarea
                class={shared.Textarea}
                spellcheck={false}
                value={props.options.initSql}
                onInput={event => {
                  const value = event.currentTarget.value;
                  props.update(options => { options.initSql = value; });
                }}
                placeholder="SET statement_timeout = '30s';"
              />
            </Field>
          </div>
        </Match>
      </SwitchCase>
    </div>
  );
}

export default PostgresConnectionForm;
