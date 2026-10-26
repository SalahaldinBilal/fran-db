import styles from "./Views.module.scss";
import { For, Show, createMemo } from "solid-js";
import { Minus, Plus } from "lucide-solid";
import { ColumnMeta } from "@core/types";
import Button from "../../Button/Button";
import { tooltip } from "../../Tooltip/Tooltip";
import Facts, { Fact } from "./Facts";
import {
  bitWidth, clamp, countBits, digitCounts, groupDigits, inRange, integerInfo, isDecimal, isFloat, isInteger,
  scientific, toBinary, toHex,
} from "./numbers";

export type NumberViewProps = {
  column: ColumnMeta,
  value: string,
  editable: boolean,
  onChange: (text: string) => void,
};

function NumberView(props: NumberViewProps) {
  const category = createMemo(() => props.column.category);
  const integer = createMemo(() => (category() === "integer" ? integerInfo(props.column.typeName) : null));
  const text = createMemo(() => props.value.trim());

  const valid = createMemo(() => {
    const current = text();
    if (category() === "decimal") return isDecimal(current);
    if (category() === "float") return isFloat(current);
    return isInteger(current);
  });

  const whole = createMemo(() => (valid() && (category() === "integer" || category() === "bit") ? BigInt(text()) : null));

  const problem = createMemo(() => {
    if (text() === "") return null;
    if (!valid()) return `Not a valid ${props.column.typeName} value`;
    const value = whole();
    const info = integer();
    if (value !== null && info && !inRange(value, info)) return `Out of range for ${props.column.typeName}`;
    if (value !== null && category() === "bit" && value < 0n) return "BIT values can't be negative";
    return null;
  });

  const rangeNote = createMemo(() => {
    const info = integer();
    if (!info) return null;
    const range = `${info.min.toLocaleString()} to ${info.max.toLocaleString()}`;
    return `${props.column.typeName} holds ${range}${info.bits === null ? ", or 0" : ""}`;
  });

  const facts = createMemo<Fact[]>(() => {
    if (!valid() || problem()) return [];
    const current = text();
    const value = whole();

    if (value !== null) {
      const bits = category() === "bit" ? bitWidth(value) : integer()?.bits ?? null;
      const items: Fact[] = [];
      if (category() === "integer") items.push({ label: "Formatted", value: groupDigits(current) });
      const hex = integer()?.bits === null ? null : toHex(value, bits);
      const binary = integer()?.bits === null ? null : toBinary(value, bits);
      if (hex) items.push({ label: "Hex", value: hex });
      if (binary) items.push({ label: "Binary", value: binary });
      if (category() === "bit") items.push({ label: "Bits set", value: `${countBits(value)} of ${bits}`, plain: true });
      return items;
    }

    const items: Fact[] = [];
    if (!/e/i.test(current)) items.push({ label: "Formatted", value: groupDigits(current) });
    items.push({ label: "Scientific", value: scientific(current) });
    if (category() === "decimal") {
      const digits = digitCounts(current);
      items.push({ label: "Digits", value: `${digits.whole} before the point, ${digits.fraction} after`, plain: true });
    }
    return items;
  });

  const bitCells = createMemo(() => {
    const value = whole();
    if (category() !== "bit" || value === null || value < 0n) return [];
    const width = bitWidth(value);
    const bytes: number[][] = [];
    for (let high = width - 1; high >= 0; high -= 8) {
      bytes.push(Array.from({ length: 8 }, (_, offset) => high - offset));
    }
    return bytes;
  });

  function step(delta: bigint) {
    const info = integer();
    const value = whole() ?? 0n;
    const next = value + delta;
    if (info) props.onChange(String(clamp(next, info)));
    else props.onChange(String(next < 0n && category() === "bit" ? 0n : next));
  }

  function toggleBit(index: number) {
    const value = whole();
    if (value === null || !props.editable) return;
    props.onChange(String(value ^ (1n << BigInt(index))));
  }

  return (
    <div class={styles.View}>
      <div class={styles.ValueRow}>
        <input
          class={styles.BigInput}
          classList={{ [styles.Invalid]: problem() !== null }}
          value={props.value}
          placeholder={props.editable ? "Type a number" : "NULL"}
          readOnly={!props.editable}
          spellcheck={false}
          autocomplete="off"
          inputMode="decimal"
          ref={input => {
            if (props.editable) queueMicrotask(() => input.focus());
          }}
          onInput={event => props.onChange(event.currentTarget.value)}
          onKeyDown={event => {
            if (!integer() || !props.editable) return;
            if (event.key === "ArrowUp" || event.key === "ArrowDown") {
              event.preventDefault();
              step(event.key === "ArrowUp" ? 1n : -1n);
            }
          }}
        />
        <Show when={props.editable && (integer() !== null || category() === "bit")}>
          <div class={styles.Stepper}>
            <Button icon variant="subtle" tooltip="Decrease (Down)" onClick={() => step(-1n)}>
              <Minus size={14} />
            </Button>
            <Button icon variant="subtle" tooltip="Increase (Up)" onClick={() => step(1n)}>
              <Plus size={14} />
            </Button>
          </div>
        </Show>
      </div>

      <Show when={problem() ?? rangeNote()}>
        {note => <div class={styles.Note} classList={{ [styles.NoteProblem]: problem() !== null }}>{note()}</div>}
      </Show>

      <Show when={bitCells().length > 0}>
        <div class={styles.Bits} classList={{ [styles.ReadOnly]: !props.editable }}>
          <For each={bitCells()}>
            {byte => (
              <div class={styles.BitByte}>
                <div class={styles.BitRow}>
                  <For each={byte}>
                    {index => {
                      const set = createMemo(() => ((whole() ?? 0n) >> BigInt(index)) & 1n);
                      return (
                        <button
                          class={styles.Bit}
                          classList={{ [styles.BitSet]: set() === 1n }}
                          ref={element => tooltip(element, () => `Bit ${index}`)}
                          tabIndex={props.editable ? 0 : -1}
                          onClick={() => toggleBit(index)}
                        >
                          {set().toString()}
                        </button>
                      );
                    }}
                  </For>
                </div>
                <div class={styles.BitLabels}>
                  <span>{byte[0]}</span>
                  <span>{byte[7]}</span>
                </div>
              </div>
            )}
          </For>
        </div>
      </Show>

      <Show when={facts().length > 0}>
        <Facts items={facts()} />
      </Show>
    </div>
  );
}

export default NumberView;
