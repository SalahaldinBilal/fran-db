import { Match, Switch } from "solid-js";
import { SshAuthMethod, SshOptions } from "@core/types";
import { SecretEditor } from "../types";
import Field, { FieldRow, FieldSection } from "@core/components/Field/Field";
import Input from "@core/components/Input/Input";
import Select, { SelectItem } from "@core/components/Select/Select";
import SecretField from "@core/components/SecretField/SecretField";
import FilePathInput from "@core/components/FilePathInput/FilePathInput";
import { toPort } from "./fields";
import { IS_WINDOWS } from "@core/helpers/platform";

const SSH_AUTH: SelectItem<SshAuthMethod>[] = [
  { value: "password", label: "Password" },
  { value: "publicKey", label: "Key file" },
  { value: "agent", label: "SSH agent" },
];

const AGENT_SOCKET_PLACEHOLDER = IS_WINDOWS ? "\\\\.\\pipe\\openssh-ssh-agent" : "$SSH_AUTH_SOCK";
const AGENT_SOCKET_HINT = IS_WINDOWS
  ? "Leave empty for the OpenSSH agent, or Pageant if that isn't running. Enter a pipe for agents like 1Password that use their own."
  : "Leave empty to use SSH_AUTH_SOCK. Enter a socket path for agents like 1Password that use their own.";

export type SshSectionProps = {
  ssh: SshOptions,
  description: string,
  secrets: SecretEditor,
  update: (mutate: (ssh: SshOptions) => void) => void,
};

function SshSection(props: SshSectionProps) {
  return (
    <FieldSection title="SSH" description={props.description}>
      <FieldRow>
        <Field label="SSH host" grow>
          <Input
            value={props.ssh.host}
            onValue={value => props.update(ssh => { ssh.host = value; })}
            placeholder="bastion.example.com"
          />
        </Field>
        <Field label="Port" width={96}>
          <Input
            type="number"
            value={props.ssh.port}
            onValue={value => props.update(ssh => { ssh.port = toPort(value); })}
          />
        </Field>
      </FieldRow>
      <FieldRow>
        <Field label="SSH user" grow>
          <Input
            value={props.ssh.user}
            onValue={value => props.update(ssh => { ssh.user = value; })}
          />
        </Field>
        <Field label="Authentication" width={160}>
          <Select
            value={props.ssh.auth}
            items={SSH_AUTH}
            onChange={auth => props.update(ssh => { ssh.auth = auth; })}
          />
        </Field>
      </FieldRow>
      <Switch>
        <Match when={props.ssh.auth === "password"}>
          <Field label="SSH password">
            <SecretField slot="sshPassword" secrets={props.secrets} />
          </Field>
        </Match>
        <Match when={props.ssh.auth === "publicKey"}>
          <Field label="Private key file">
            <FilePathInput
              value={props.ssh.keyFile}
              onValue={value => props.update(ssh => { ssh.keyFile = value; })}
              placeholder="~/.ssh/id_ed25519"
              dialogTitle="Choose an SSH private key"
            />
          </Field>
          <Field label="Key passphrase" hint="Only needed for encrypted keys.">
            <SecretField slot="sshKeyPassphrase" secrets={props.secrets} />
          </Field>
        </Match>
        <Match when={props.ssh.auth === "agent"}>
          <Field label="Agent socket" hint={AGENT_SOCKET_HINT}>
            <Input
              value={props.ssh.agentSocket}
              onValue={value => props.update(ssh => { ssh.agentSocket = value; })}
              placeholder={AGENT_SOCKET_PLACEHOLDER}
            />
          </Field>
        </Match>
      </Switch>
    </FieldSection>
  );
}

export default SshSection;
