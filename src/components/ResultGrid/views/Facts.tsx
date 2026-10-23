import styles from "./Views.module.scss";
import { For } from "solid-js";
import { Copy } from "lucide-solid";
import { copyText } from "@core/helpers/clipboard";

export type Fact = { label: string, value: string, plain?: boolean };

function Facts(props: { items: Fact[] }) {
  return (
    <dl class={styles.Facts}>
      <For each={props.items}>
        {fact => (
          <div class={styles.Fact}>
            <dt>{fact.label}</dt>
            <dd classList={{ [styles.Plain]: fact.plain ?? false }}>{fact.value}</dd>
            <button
              class={styles.CopyFact}
              title={`Copy ${fact.label.toLowerCase()}`}
              onClick={() => copyText(fact.value, fact.label.toLowerCase())}
            >
              <Copy size={12} />
            </button>
          </div>
        )}
      </For>
    </dl>
  );
}

export default Facts;
