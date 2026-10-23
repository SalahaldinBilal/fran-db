import styles from "./SecretField.module.scss";
import { Show, createMemo } from "solid-js";
import { KeyRound, Undo2 } from "lucide-solid";
import Input from "../Input/Input";
import Button from "../Button/Button";
import { SecretEditor } from "@core/databases/types";

export type SecretFieldProps = {
  slot: string,
  secrets: SecretEditor,
};

function SecretField(props: SecretFieldProps) {
  const stored = createMemo(() => props.secrets.isStored(props.slot));
  const pending = createMemo(() => props.secrets.pending(props.slot));

  return (
    <Show
      when={pending() !== null}
      fallback={
        <div class={styles.Removed}>
          <span>Removed from the vault on save</span>
          <Button size="sm" onClick={() => props.secrets.setPending(props.slot, undefined)}>
            <Undo2 size={14} />
            Undo
          </Button>
        </div>
      }
    >
      <Input
        type="password"
        value={pending() ?? ""}
        onValue={value => props.secrets.setPending(props.slot, value === "" ? undefined : value)}
        placeholder={stored() ? "Saved in vault" : "Asked when connecting"}
        before={<KeyRound size={14} class={stored() ? styles.StoredIcon : undefined} />}
        style={{ "padding-right": stored() ? "3px" : undefined }}
        after={
          <Show when={stored() && pending() === undefined}>
            <Button size="sm" variant="danger" onClick={() => props.secrets.setPending(props.slot, null)}>
              Remove
            </Button>
          </Show>
        }
      />
    </Show>
  );
}

export default SecretField;
