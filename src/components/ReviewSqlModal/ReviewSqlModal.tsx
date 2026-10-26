import styles from "./ReviewSqlModal.module.scss";
import { Show, createComputed, createMemo, createSignal, on } from "solid-js";
import { CircleAlert, ShieldCheck, TriangleAlert } from "lucide-solid";
import { SQLDialect } from "@codemirror/lang-sql";
import { plural } from "@core/helpers";
import Modal from "../Modal/Modal";
import Button from "../Button/Button";
import SqlEditor from "../SqlEditor/SqlEditor";
import Spinner from "../Spinner/Spinner";

// "transaction": the app wraps the statements in one. "schema": DDL, committed statement by statement,
// with `atomic` saying whether this server applies each one completely or not at all.
export type ApplySafety = { kind: "transaction" } | { kind: "schema", atomic: boolean };

export type ReviewSqlModalProps = {
  statements: string[] | null,
  title: string,
  description: string,
  dialect: SQLDialect,
  safety: ApplySafety,
  applyLabel?: string,
  cancelLabel?: string,
  // Resolves to an error message to show, or null when everything ran.
  onApply: () => Promise<string | null>,
  onClose: () => void,
};

export type SafetyNote = { safe: boolean, text: string };

export function safetyNote(safety: ApplySafety, count: number): SafetyNote {
  if (safety.kind === "transaction") {
    return { safe: true, text: "Runs in a single transaction. If any statement fails, everything is rolled back and nothing changes." };
  }
  if (safety.atomic && count <= 1) {
    return { safe: true, text: "Table changes can't run in a transaction, but this is one statement and this server applies it completely or not at all." };
  }
  if (safety.atomic) {
    return {
      safe: false,
      text: `Table changes can't run in a transaction. Each of these ${plural(count, "statement")} is applied completely or not at all, but if one fails, the ones before it stay applied.`,
    };
  }
  return {
    safe: false,
    text: "Table changes can't run in a transaction, and this server version can leave a failed statement half applied. Statements run in order and stop at the first error.",
  };
}

function ReviewSqlModal(props: ReviewSqlModalProps) {
  const [applying, setApplying] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const script = createMemo(() => (props.statements ?? []).map(statement => `${statement};`).join("\n\n"));

  createComputed(on(() => props.statements, () => setError(null)));

  const note = createMemo(() => safetyNote(props.safety, props.statements?.length ?? 0));

  async function apply() {
    setApplying(true);
    setError(null);
    try {
      const failure = await props.onApply();
      if (failure) setError(failure);
      else props.onClose();
    } finally {
      setApplying(false);
    }
  }

  return (
    <Modal
      show={props.statements !== null}
      onHide={() => {
        if (!applying()) props.onClose();
      }}
      title={props.title}
      subtitle={props.description}
      width={760}
      footer={
        <>
          <Button variant="subtle" onClick={props.onClose} disabled={applying()}>{props.cancelLabel ?? "Cancel"}</Button>
          <Button variant="primary" onClick={apply} disabled={applying()}>
            <Show when={applying()}><Spinner size={12} /></Show>
            {props.applyLabel ?? "Apply"}
          </Button>
        </>
      }
    >
      <div class={styles.Body}>
        <Show when={props.statements} keyed>
          <div class={styles.Script}>
            <SqlEditor initialValue={script()} dialect={props.dialect} readOnly wrap />
          </div>
        </Show>
        <div class={styles.Safety} classList={{ [styles.Unsafe]: !note().safe }}>
          {note().safe ? <ShieldCheck size={14} /> : <TriangleAlert size={14} />}
          <span>{note().text}</span>
        </div>
        <Show when={error()}>
          <div class={styles.Error}>
            <CircleAlert size={14} />
            <span>{error()}</span>
          </div>
        </Show>
      </div>
    </Modal>
  );
}

export default ReviewSqlModal;
