import styles from "./Prompts.module.scss";
import { For, Show, createEffect, createMemo, createSignal } from "solid-js";
import { createStore } from "solid-js/store";
import { ShieldAlert, ShieldQuestionMark } from "lucide-solid";
import usePromptState from "@core/states/promptState";
import Modal from "../Modal/Modal";
import Button from "../Button/Button";
import Field from "../Field/Field";
import Input from "../Input/Input";
import Switch from "../Switch/Switch";

function SecretsPrompt() {
  const { secretsRequest } = usePromptState;
  const [values, setValues] = createStore<Record<string, string>>({});
  const [save, setSave] = createSignal(false);

  createEffect(() => {
    if (!secretsRequest()) return;
    setValues(Object.fromEntries(secretsRequest()!.slots.map(slot => [slot.id, ""])));
    setSave(false);
  });

  function submit() {
    secretsRequest()?.resolve({ values: { ...values }, save: save() });
  }

  return (
    <Modal
      show={secretsRequest() !== null}
      onHide={() => secretsRequest()?.resolve(null)}
      title="Credentials needed"
      subtitle={secretsRequest()?.connectionName}
      width={420}
      footer={
        <>
          <Button variant="subtle" onClick={() => secretsRequest()?.resolve(null)}>Cancel</Button>
          <Button variant="primary" onClick={submit}>Connect</Button>
        </>
      }
    >
      <form
        class={styles.Form}
        onSubmit={event => {
          event.preventDefault();
          submit();
        }}
      >
        <For each={secretsRequest()?.slots ?? []}>
          {(slot, index) => (
            <Field label={slot.label}>
              <Input
                type="password"
                autofocus={index() === 0}
                value={values[slot.id] ?? ""}
                onValue={value => setValues(slot.id, value)}
              />
            </Field>
          )}
        </For>
        <button type="submit" hidden />
        <Show when={secretsRequest()?.canSave}>
          <Switch checked={save()} onChange={setSave} label="Save in the vault" />
        </Show>
      </form>
    </Modal>
  );
}

function HostKeyPrompt() {
  const { hostKeyRequest } = usePromptState;
  const changed = createMemo(() => hostKeyRequest()?.knownFingerprint != null);

  return (
    <Modal
      show={hostKeyRequest() !== null}
      onHide={() => hostKeyRequest()?.resolve(false)}
      title={changed() ? "SSH host key changed" : "Unknown SSH host"}
      width={520}
      footer={
        <>
          <Button variant="subtle" onClick={() => hostKeyRequest()?.resolve(false)}>Cancel</Button>
          <Button variant={changed() ? "danger" : "primary"} onClick={() => hostKeyRequest()?.resolve(true)}>
            {changed() ? "Replace and connect" : "Trust and connect"}
          </Button>
        </>
      }
    >
      <Show when={hostKeyRequest()}>
        {request => (
          <div class={styles.HostKey}>
            <div class={styles.HostKeyLead} classList={{ [styles.Danger]: changed() }}>
              <Show when={changed()} fallback={<ShieldQuestionMark size={20} />}>
                <ShieldAlert size={20} />
              </Show>
              <p>
                <Show
                  when={changed()}
                  fallback={<>This is the first connection to <b>{request().key.host}:{request().key.port}</b>. Check the fingerprint with the server admin before trusting it.</>}
                >
                  The key presented by <b>{request().key.host}:{request().key.port}</b> doesn't match the one saved earlier.
                  The server may have been reinstalled, or someone may be intercepting the connection.
                </Show>
              </p>
            </div>
            <dl class={styles.Fingerprints}>
              <dt>Key type</dt>
              <dd>{request().key.algorithm}</dd>
              <dt>{changed() ? "New fingerprint" : "Fingerprint"}</dt>
              <dd>{request().key.fingerprint}</dd>
              <Show when={request().knownFingerprint}>
                {known => (
                  <>
                    <dt>Saved fingerprint</dt>
                    <dd>{known()}</dd>
                  </>
                )}
              </Show>
            </dl>
          </div>
        )}
      </Show>
    </Modal>
  );
}

function ConfirmPrompt() {
  const { confirmRequest } = usePromptState;

  return (
    <Modal
      show={confirmRequest() !== null}
      onHide={() => confirmRequest()?.resolve(false)}
      title={confirmRequest()?.title}
      width={420}
      footer={
        <>
          <Button variant="subtle" onClick={() => confirmRequest()?.resolve(false)}>Cancel</Button>
          <Button
            variant={confirmRequest()?.danger ? "danger" : "primary"}
            onClick={() => confirmRequest()?.resolve(true)}
          >
            {confirmRequest()?.confirmLabel}
          </Button>
        </>
      }
    >
      <p class={styles.Message}>{confirmRequest()?.message}</p>
    </Modal>
  );
}

function TextPrompt() {
  const { textRequest } = usePromptState;
  const [value, setValue] = createSignal("");

  createEffect(() => {
    const request = textRequest();
    if (request) setValue(request.initial);
  });

  function submit() {
    const trimmed = value().trim();
    if (trimmed) textRequest()?.resolve(trimmed);
  }

  return (
    <Modal
      show={textRequest() !== null}
      onHide={() => textRequest()?.resolve(null)}
      title={textRequest()?.title}
      width={400}
      footer={
        <>
          <Button variant="subtle" onClick={() => textRequest()?.resolve(null)}>Cancel</Button>
          <Button variant="primary" onClick={submit} disabled={!value().trim()}>
            {textRequest()?.confirmLabel}
          </Button>
        </>
      }
    >
      <form
        class={styles.Form}
        onSubmit={event => {
          event.preventDefault();
          submit();
        }}
      >
        <Field label={textRequest()?.label ?? ""}>
          <Input value={value()} onValue={setValue} autofocus />
        </Field>
        <button type="submit" hidden />
      </form>
    </Modal>
  );
}

function Prompts() {
  return (
    <>
      <SecretsPrompt />
      <HostKeyPrompt />
      <ConfirmPrompt />
      <TextPrompt />
    </>
  );
}

export default Prompts;
