import styles from "./PortForwardModal.module.scss";
import { Show, createEffect, createMemo, createSignal } from "solid-js";
import { withDatabaseModule } from "@core/databases";
import usePortForwardState from "@core/states/portForwardState";
import useToastState from "@core/states/toastState";
import { toPort } from "@core/databases/shared/fields";
import Modal from "../Modal/Modal";
import Button from "../Button/Button";
import Field from "../Field/Field";
import Input from "../Input/Input";

function PortForwardModal() {
  const forwards = usePortForwardState;
  const { pushError } = useToastState;
  const [port, setPort] = createSignal("");
  const [busy, setBusy] = createSignal(false);

  const profile = createMemo(() => forwards.dialogProfile());
  const running = createMemo(() => {
    const current = profile();
    return current ? forwards.active(current.id) : undefined;
  });
  const target = createMemo(() => {
    const current = profile();
    return current ? withDatabaseModule(current, (module, options) => module.sshTarget(options)) : null;
  });
  const localPort = createMemo(() => toPort(port()));

  createEffect(() => {
    const current = profile();
    if (current) setPort(String(forwards.active(current.id)?.localPort ?? current.forwardPort ?? target()?.port ?? ""));
  });

  async function start() {
    const current = profile();
    if (!current || localPort() === 0) return;
    setBusy(true);
    try {
      await forwards.start(current, localPort());
    } catch (error) {
      pushError(error);
    } finally {
      setBusy(false);
    }
  }

  async function stop() {
    const current = profile();
    if (!current) return;
    setBusy(true);
    try {
      await forwards.stop(current.id);
    } catch (error) {
      pushError(error);
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal
      show={profile() !== null}
      onHide={forwards.closeDialog}
      title="Port forward"
      subtitle={profile()?.name}
      width={420}
      footer={
        <>
          <Button variant="subtle" onClick={forwards.closeDialog}>Close</Button>
          <Show
            when={running()}
            fallback={
              <Button variant="primary" onClick={start} disabled={busy() || localPort() === 0}>
                Start
              </Button>
            }
          >
            <Button variant="danger" onClick={stop} disabled={busy()}>Stop</Button>
          </Show>
        </>
      }
    >
      <form
        class={styles.Form}
        onSubmit={event => {
          event.preventDefault();
          if (!running()) void start();
        }}
      >
        <p class={styles.Message}>
          <Show when={running()} fallback={<><code>local:{localPort() || "?"}</code> → {profile()?.name} → {target()?.host}:{target()?.port}</>}>
            {info => <>Listening on <code>local:{info().localPort}</code> → {profile()?.name} → {info().host}:{info().port}</>}
          </Show>
        </p>
        <Field label="Local port" width={160}>
          <Input type="number" min={1} max={65535} value={port()} onValue={setPort} readOnly={running() !== undefined} autofocus />
        </Field>
        <button type="submit" hidden />
      </form>
    </Modal>
  );
}

export default PortForwardModal;
