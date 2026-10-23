import styles from "./PendingChanges.module.scss";
import { Check } from "lucide-solid";
import { plural } from "@core/helpers";

export type PendingChangesProps = {
  count: number,
  onDiscard: () => void,
  onApply: () => void,
};

function PendingChanges(props: PendingChangesProps) {
  return (
    <div class={styles.PendingChanges}>
      <span class={styles.Label}>
        <span class={styles.Dot} />
        {plural(props.count, "change")}
      </span>
      <button class={styles.Discard} onClick={() => props.onDiscard()}>Discard</button>
      <button class={styles.Apply} onClick={() => props.onApply()}>
        <Check size={14} strokeWidth={2.75} />
        Apply
      </button>
    </div>
  );
}

export default PendingChanges;
