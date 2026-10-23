import styles from "./Views.module.scss";
import { For, Show, createMemo, createSignal, onMount } from "solid-js";
import { Check, Search } from "lucide-solid";
import { ColumnChoices } from "@core/types";
import { plural } from "@core/helpers";

export type ChoiceViewProps = {
  choices: ColumnChoices,
  value: string,
  original: string | null,
  isNull: boolean,
  editable: boolean,
  onChange: (text: string) => void,
};

const FILTER_FROM = 12;

function sameChoice(a: string, b: string): boolean {
  return a.localeCompare(b, undefined, { sensitivity: "accent" }) === 0;
}

function splitValue(text: string, multiple: boolean): string[] {
  return multiple ? text.split(",").filter(part => part !== "") : [text];
}

function ChoiceView(props: ChoiceViewProps) {
  const [query, setQuery] = createSignal("");
  let list!: HTMLDivElement;

  const picked = createMemo(() => (props.isNull ? [] : splitValue(props.value, props.choices.multiple)));
  const originalPicked = createMemo(() => (props.original === null ? [] : splitValue(props.original, props.choices.multiple)));
  const changed = createMemo(() => props.isNull ? props.original !== null : props.value !== (props.original ?? ""));

  const unknown = createMemo(() => picked().filter(value => !props.choices.values.some(choice => sameChoice(choice, value))));

  const shown = createMemo(() => {
    const needle = query().trim().toLowerCase();
    return props.choices.values
      .map((choice, index) => ({ choice, number: index + 1 }))
      .filter(entry => !needle || entry.choice.toLowerCase().includes(needle));
  });

  const caption = createMemo(() => {
    const count = plural(props.choices.values.length, "value");
    if (!props.editable) return props.choices.multiple ? `Any of ${count}` : `One of ${count}`;
    return props.choices.multiple ? `Pick any of ${count}` : `Pick one of ${count}`;
  });

  function isPicked(choice: string): boolean {
    return picked().some(value => sameChoice(value, choice));
  }

  function wasPicked(choice: string): boolean {
    return originalPicked().some(value => sameChoice(value, choice));
  }

  function pick(choice: string) {
    if (!props.editable) return;
    if (!props.choices.multiple) {
      props.onChange(choice);
      return;
    }
    const next = isPicked(choice) ? picked().filter(value => !sameChoice(value, choice)) : [...picked(), choice];
    props.onChange(props.choices.values.filter(value => next.some(item => sameChoice(item, value))).join(","));
  }

  function arrowStep(key: string): number {
    const columns = getComputedStyle(list).gridTemplateColumns.split(" ").length;
    const steps: Record<string, number> = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -columns, ArrowDown: columns };
    return steps[key] ?? 0;
  }

  function onKeyDown(event: KeyboardEvent) {
    if (event.target instanceof HTMLInputElement || event.ctrlKey || event.metaKey || event.altKey) return;

    if (/^[1-9]$/.test(event.key)) {
      const choice = props.choices.values[Number(event.key) - 1];
      if (choice !== undefined) {
        event.preventDefault();
        pick(choice);
      }
      return;
    }

    const step = arrowStep(event.key);
    if (step === 0) return;
    event.preventDefault();
    const buttons = Array.from(list.querySelectorAll<HTMLButtonElement>("button"));
    const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
    const next = buttons[Math.min(buttons.length - 1, Math.max(0, index + step))];
    next?.focus();
    if (next && !props.choices.multiple) pick(next.dataset.choice ?? "");
  }

  // Scrolls only the list, so opening the modal never moves anything else.
  onMount(() => {
    const first = list.querySelector<HTMLElement>(`.${styles.Picked}`);
    if (first) list.scrollTop = first.offsetTop - list.clientHeight / 2 + first.offsetHeight / 2;
    if (props.editable) (first ?? list.querySelector<HTMLElement>("button"))?.focus({ preventScroll: true });
  });

  return (
    <div class={styles.ChoiceView} onKeyDown={onKeyDown}>
      <div class={styles.ChoiceHeader}>
        <span class={styles.ChoiceCaption}>
          {caption()}
          <Show when={props.choices.multiple && !props.isNull}>
            <span class={styles.ChoiceCount}>{picked().length} picked</span>
          </Show>
        </span>
        <Show when={props.choices.values.length >= FILTER_FROM}>
          <label class={styles.ChoiceFilter}>
            <Search size={12} />
            <input value={query()} placeholder="Filter values" spellcheck={false} onInput={event => setQuery(event.currentTarget.value)} />
          </label>
        </Show>
      </div>

      <Show when={unknown().length > 0}>
        <div class={`${styles.Note} ${styles.NoteProblem}`}>
          {unknown().map(value => `"${value}"`).join(", ")} {unknown().length === 1 ? "isn't" : "aren't"} in the column's list.
        </div>
      </Show>

      <div
        ref={list}
        class={styles.Choices}
        classList={{ [styles.ReadOnly]: !props.editable }}
        role={props.choices.multiple ? "group" : "radiogroup"}
      >
        <For each={shown()} fallback={<div class={styles.NoChoices}>No value contains "{query().trim()}"</div>}>
          {entry => (
            <button
              class={styles.Choice}
              classList={{
                [styles.Picked]: isPicked(entry.choice),
                [styles.Was]: changed() && wasPicked(entry.choice) && !isPicked(entry.choice),
              }}
              role={props.choices.multiple ? "checkbox" : "radio"}
              aria-checked={isPicked(entry.choice)}
              data-choice={entry.choice}
              title={entry.choice === "" ? "Empty string" : entry.choice}
              tabIndex={props.editable || isPicked(entry.choice) ? 0 : -1}
              onClick={() => pick(entry.choice)}
            >
              <Show when={props.choices.multiple}>
                <span class={styles.ChoiceBox}>
                  <Check size={12} strokeWidth={3} />
                </span>
              </Show>
              <span class={styles.ChoiceLabel} classList={{ [styles.EmptyChoice]: entry.choice === "" }}>
                {entry.choice === "" ? "empty string" : entry.choice}
              </span>
              <Show
                when={isPicked(entry.choice) && !props.choices.multiple}
                fallback={
                  <Show when={changed() && wasPicked(entry.choice)} fallback={<span class={styles.ChoiceNumber}>{entry.number}</span>}>
                    <span class={styles.WasTag}>current</span>
                  </Show>
                }
              >
                <span class={styles.PickedMark}>
                  <Check size={12} strokeWidth={3} />
                </span>
              </Show>
            </button>
          )}
        </For>
      </div>
    </div>
  );
}

export default ChoiceView;
