import shared from "../shared/ConnectionForm.module.scss";
import styles from "./MysqlConnectionForm.module.scss";
import { Match, Show, Switch as SwitchCase, createMemo, createSignal } from "solid-js";
import { Cable, KeyRound, Network, Plug } from "lucide-solid";
import { MysqlConnectMethod, MysqlOptions, SslMode } from "@core/types";
import { ConnectionFormProps } from "../types";
import Field, { FieldRow, FieldSection } from "@core/components/Field/Field";
import Input from "@core/components/Input/Input";
import Select, { SelectItem } from "@core/components/Select/Select";
import Switch from "@core/components/Switch/Switch";
import SegmentedControl from "@core/components/SegmentedControl/SegmentedControl";
import SecretField from "@core/components/SecretField/SecretField";
import SshSection from "../shared/SshSection";
import SslFileFields from "../shared/SslFileFields";
import { IS_WINDOWS, toPort, toTimeout } from "../shared/fields";

type Section = "connection" | "ssl" | "advanced";

const METHODS = [
  { value: "tcp", label: "TCP/IP", icon: Network },
  { value: "socket", label: IS_WINDOWS ? "Named pipe" : "Socket", icon: Plug },
  { value: "sshTunnel", label: "TCP/IP over SSH", icon: Cable },
] as const satisfies readonly { value: MysqlConnectMethod, label: string, icon: typeof Network }[];

const SSL_MODES: SelectItem<SslMode>[] = [
  { value: "disabled", label: "Off", hint: "Plain connection" },
  { value: "preferred", label: "If available", hint: "Encrypt when the server supports it" },
  { value: "required", label: "Required", hint: "Encrypt, don't check the certificate" },
  { value: "verifyCa", label: "Verify CA", hint: "Certificate must be signed by the CA" },
  { value: "verifyIdentity", label: "Verify identity", hint: "CA plus host name match" },
];

function MysqlConnectionForm(props: ConnectionFormProps<MysqlOptions>) {
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
                description="The tunnel opens first, then MySQL is reached from the SSH server."
                secrets={props.secrets}
                update={mutate => props.update(options => mutate(options.ssh))}
              />
            </Show>

            <FieldSection title="MySQL">
              <Show
                when={!isSocket()}
                fallback={
                  <Field
                    label={IS_WINDOWS ? "Pipe name" : "Socket path"}
                    hint={IS_WINDOWS ? "The server's named_pipe name, usually MySQL." : undefined}
                  >
                    <Input
                      mono
                      value={props.options.socketPath}
                      onValue={value => props.update(options => { options.socketPath = value; })}
                      placeholder={IS_WINDOWS ? "MySQL" : "/var/run/mysqld/mysqld.sock"}
                    />
                  </Field>
                }
              >
                <FieldRow>
                  <Field label={isSsh() ? "MySQL host (from the SSH server)" : "Host"} grow>
                    <Input
                      value={props.options.host}
                      onValue={value => props.update(options => { options.host = value; })}
                      placeholder="127.0.0.1"
                    />
                  </Field>
                  <Field label="Port" width={96}>
                    <Input
                      type="number"
                      value={props.options.port}
                      onValue={value => props.update(options => { options.port = toPort(value); })}
                    />
                  </Field>
                </FieldRow>
              </Show>
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
              <Field label="Default schema" hint="Opened automatically after connecting. Leave empty for none.">
                <Input
                  value={props.options.defaultSchema}
                  onValue={value => props.update(options => { options.defaultSchema = value; })}
                />
              </Field>
            </FieldSection>
          </div>
        </Match>

        <Match when={section() === "ssl"}>
          <div class={shared.Sections}>
            <Show when={isSocket()}>
              <div class={shared.Note}>SSL isn't used for socket and named pipe connections.</div>
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
            <div class={styles.Toggles}>
              <Switch
                checked={props.options.compression}
                onChange={checked => props.update(options => { options.compression = checked; })}
                label="Use compression"
              />
              <div class={styles.ToggleWithHint}>
                <Switch
                  checked={props.options.cleartextAuth}
                  onChange={checked => props.update(options => { options.cleartextAuth = checked; })}
                  label="Enable cleartext authentication"
                />
                <span class={styles.Warning}>
                  <KeyRound size={12} />
                  Sends the password unencrypted unless SSL is on. Needed for some LDAP and PAM setups.
                </span>
              </div>
            </div>
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
                placeholder="SET SESSION sql_mode = 'TRADITIONAL';"
              />
            </Field>
          </div>
        </Match>
      </SwitchCase>
    </div>
  );
}

export default MysqlConnectionForm;
