import styles from "./Views.module.scss";
import { For, Show, createEffect, createMemo, createSignal, on, untrack } from "solid-js";
import { ChevronLeft, ChevronRight, Clock } from "lucide-solid";
import { ColumnMeta } from "@core/types";
import Button from "../../Button/Button";
import { tooltip } from "../../Tooltip/Tooltip";
import Facts, { Fact } from "./Facts";
import {
  DateParts, TemporalValue, TimeParts, canonicalTemporal, currentTemporal, dayOfYear, describeDuration, formatDate,
  formatTemporal, formatTime, fractionDigitsOf, isZeroDate, isZonedType, isoWeek, midnight, monthGrid, readTemporal,
  relativeTo, sameDate, shiftMonth, temporalKind, today, totalSeconds, unixSeconds, utcDate,
} from "./temporal";

export type TemporalViewProps = {
  column: ColumnMeta,
  value: string,
  editable: boolean,
  onChange: (text: string) => void,
};

const longDate = new Intl.DateTimeFormat(undefined, { weekday: "long", year: "numeric", month: "long", day: "numeric", timeZone: "UTC" });
const monthTitle = new Intl.DateTimeFormat(undefined, { year: "numeric", month: "long", timeZone: "UTC" });
const weekdayName = new Intl.DateTimeFormat(undefined, { weekday: "short", timeZone: "UTC" });
const WEEKDAYS = Array.from({ length: 7 }, (_, index) => weekdayName.format(utcDate({ year: 2024, month: 1, day: 1 + index })));

type SegmentProps = {
  label: string,
  value: number,
  max: number,
  digits?: number,
  editable: boolean,
  onCommit: (value: number) => void,
};

function Segment(props: SegmentProps) {
  const digits = createMemo(() => props.digits ?? 2);
  const display = createMemo(() => String(props.value).padStart(digits(), "0"));

  function commit(input: HTMLInputElement, next: number) {
    const clamped = Math.min(props.max, Math.max(0, next));
    if (clamped !== props.value) props.onCommit(clamped);
    input.value = String(clamped).padStart(digits(), "0");
  }

  return (
    <input
      class={styles.Segment}
      style={{ width: `${Math.max(digits(), String(props.max).length) + 1.6}ch` }}
      value={display()}
      aria-label={props.label}
      ref={element => tooltip(element, () => props.label)}
      inputMode="numeric"
      readOnly={!props.editable}
      spellcheck={false}
      onFocus={event => event.currentTarget.select()}
      onKeyDown={event => {
        if (!props.editable) return;
        if (event.key === "ArrowUp" || event.key === "ArrowDown") {
          event.preventDefault();
          const delta = event.key === "ArrowUp" ? 1 : -1;
          commit(event.currentTarget, (props.value + delta + props.max + 1) % (props.max + 1));
          event.currentTarget.select();
        } else if (event.key === "Enter") {
          event.currentTarget.blur();
        }
      }}
      onChange={event => {
        const parsed = Number.parseInt(event.currentTarget.value, 10);
        if (Number.isNaN(parsed)) event.currentTarget.value = display();
        else commit(event.currentTarget, parsed);
      }}
    />
  );
}

function TemporalView(props: TemporalViewProps) {
  const kind = createMemo(() => temporalKind(props.column.typeName));
  const zoned = createMemo(() => isZonedType(props.column.typeName));
  const reading = createMemo(() => readTemporal(kind(), props.value, zoned()));
  const parsed = createMemo(() => reading()?.value ?? null);
  const invalid = createMemo(() => props.value.trim() !== "" && reading() === null);
  const current = createMemo<TemporalValue | null>(previous => parsed() ?? previous ?? null, null);
  const columnFraction = untrack(() => fractionDigitsOf(kind(), props.value.trim() === "" ? null : props.value, zoned()));
  const fractionDigits = columnFraction ?? 0;

  const canonical = createMemo(() => canonicalTemporal(kind(), props.value, columnFraction, zoned()));
  const conversion = createMemo(() => {
    const text = canonical();
    if (!text || text === props.value.trim()) return null;
    const note = reading()?.note;
    return note ? `${text} (${note})` : text;
  });

  function useCanonical() {
    const text = canonical();
    if (text && text !== props.value) props.onChange(text);
  }

  function onPaste(event: ClipboardEvent & { currentTarget: HTMLInputElement }) {
    const pasted = event.clipboardData?.getData("text") ?? "";
    const input = event.currentTarget;
    const next = input.value.slice(0, input.selectionStart ?? 0) + pasted + input.value.slice(input.selectionEnd ?? input.value.length);
    const text = canonicalTemporal(kind(), next, columnFraction, zoned());
    if (!text) return;
    event.preventDefault();
    props.onChange(text);
  }

  const date = createMemo(() => current()?.date ?? null);
  const time = createMemo(() => current()?.time ?? null);
  const clock = createMemo(() => time() ?? midnight(fractionDigits));
  const zero = createMemo(() => date() !== null && isZeroDate(date()!));
  const realDate = createMemo(() => (date() && !zero() ? date() : null));

  const initialMonth = untrack(() => realDate() ?? today());
  const [view, setView] = createSignal({ year: initialMonth.year, month: initialMonth.month });

  createEffect(on(realDate, next => {
    if (next) setView({ year: next.year, month: next.month });
  }, { defer: true }));

  const days = createMemo(() => monthGrid(view().year, view().month));
  const todayDate = today();

  const facts = createMemo<Fact[]>(() => {
    const items: Fact[] = [];
    const day = realDate();
    const moment = time();
    const zone = current()?.zone ?? null;

    if (kind() === "time" && moment) {
      items.push({ label: "Duration", value: describeDuration(moment), plain: true });
      items.push({ label: "Seconds", value: totalSeconds(moment) });
      return items;
    }
    if (!day) return items;

    const isDate = kind() === "date";
    items.push({ label: "Relative", value: relativeTo(day, isDate ? null : moment, isDate ? null : zone), plain: true });
    items.push({ label: "Unix time (UTC)", value: unixSeconds(day, isDate ? null : moment, isDate ? null : zone) });
    items.push({ label: "ISO 8601", value: isDate ? formatDate(day) : `${formatDate(day)}T${formatTime(moment ?? midnight())}${zone ?? ""}` });
    const week = isoWeek(day);
    items.push({ label: "Week", value: `Week ${week.week} of ${week.year}, day ${dayOfYear(day)} of the year`, plain: true });
    return items;
  });

  function update(change: Partial<TemporalValue>) {
    const base: TemporalValue = current() ?? { date: null, time: null };
    const next: TemporalValue = { ...base, ...change };
    if (kind() !== "time" && !next.date) next.date = today();
    if (kind() !== "date" && !next.time) next.time = midnight(fractionDigits);
    props.onChange(formatTemporal(kind(), next));
  }

  function pickDay(day: DateParts) {
    if (!props.editable) return;
    update({ date: day });
  }

  function setTimePart(part: "hours" | "minutes" | "seconds", value: number) {
    const base: TimeParts = time() ?? midnight(fractionDigits);
    update({ time: { ...base, [part]: value } });
  }

  function setFraction(text: string) {
    const base: TimeParts = time() ?? midnight(fractionDigits);
    update({ time: { ...base, fraction: text.replace(/\D/g, "").padEnd(fractionDigits, "0").slice(0, fractionDigits) } });
  }

  function setNow() {
    const now = currentTemporal(fractionDigits, zoned());
    if (kind() === "date") update({ date: now.date });
    else if (kind() === "time") update({ time: now.time });
    else update(now);
  }

  function moveView(delta: number) {
    setView(shiftMonth(view().year, view().month, delta));
  }

  const nowTooltip = createMemo(() => (kind() === "date" ? "Set to today" : "Set to the current time"));
  const maxHours = createMemo(() => (kind() === "time" ? 838 : 23));

  const headline = createMemo(() => {
    if (zero()) return "Zero date";
    const day = realDate();
    return day ? longDate.format(utcDate(day)) : null;
  });

  return (
    <div class={styles.View}>
      <div class={styles.ValueRow}>
        <input
          class={styles.BigInput}
          classList={{ [styles.Invalid]: invalid() }}
          value={props.value}
          placeholder={props.editable ? formatTemporal(kind(), currentTemporal(fractionDigits, zoned())) : "NULL"}
          readOnly={!props.editable}
          spellcheck={false}
          autocomplete="off"
          ref={input => {
            if (props.editable) queueMicrotask(() => input.focus());
          }}
          onInput={event => props.onChange(event.currentTarget.value)}
          onPaste={onPaste}
          onBlur={useCanonical}
          onKeyDown={event => {
            if (event.key === "Enter" && !event.ctrlKey && !event.metaKey && conversion()) {
              event.preventDefault();
              useCanonical();
            }
          }}
        />
        <Show when={props.editable}>
          <Button variant="subtle" class={styles.NowButton} onClick={setNow} tooltip={nowTooltip()}>
            <Clock size={14} />
            {kind() === "date" ? "Today" : "Now"}
          </Button>
        </Show>
      </div>

      <Show when={invalid()}>
        <div class={`${styles.Note} ${styles.NoteProblem}`}>
          Can't read this as a {props.column.typeName}. Try {kind() === "date" ? "YYYY-MM-DD" : kind() === "time" ? "HH:MM:SS" : "YYYY-MM-DD HH:MM:SS"}, a Unix timestamp or an ISO 8601 date.
        </div>
      </Show>
      <Show when={props.editable && conversion()}>
        <div class={styles.Note}>Saved as {conversion()}. Press Enter to apply it here.</div>
      </Show>

      <div class={styles.TemporalBody} classList={{ [styles.TimeOnly]: kind() === "time" }}>
        <Show when={kind() !== "time"}>
          <div class={styles.Calendar} classList={{ [styles.ReadOnly]: !props.editable }}>
            <div class={styles.CalendarHeader}>
              <Button icon size="sm" tooltip="Previous month" onClick={() => moveView(-1)}>
                <ChevronLeft size={16} />
              </Button>
              <span class={styles.MonthTitle}>{monthTitle.format(utcDate({ ...view(), day: 1 }))}</span>
              <Button icon size="sm" tooltip="Next month" onClick={() => moveView(1)}>
                <ChevronRight size={16} />
              </Button>
            </div>
            <div class={styles.Weekdays}>
              <For each={WEEKDAYS}>{name => <span>{name}</span>}</For>
            </div>
            <div class={styles.Days}>
              <For each={days()}>
                {day => (
                  <button
                    class={styles.Day}
                    classList={{
                      [styles.Outside]: day.month !== view().month,
                      [styles.Today]: sameDate(day, todayDate),
                      [styles.SelectedDay]: sameDate(day, realDate()),
                    }}
                    tabIndex={props.editable ? 0 : -1}
                    onClick={() => pickDay(day)}
                  >
                    {day.day}
                  </button>
                )}
              </For>
            </div>
          </div>
        </Show>

        <div class={styles.Summary}>
          <Show when={headline()}>
            <div class={styles.Headline} classList={{ [styles.Muted]: zero() }}>{headline()}</div>
          </Show>
          <Show when={kind() !== "date"}>
            <div class={styles.Segments} classList={{ [styles.Muted]: time() === null }}>
              <Show when={clock().negative}><span class={styles.Sign}>-</span></Show>
              <Segment label="Hours" value={clock().hours} max={maxHours()} editable={props.editable} onCommit={value => setTimePart("hours", value)} />
              <span class={styles.Colon}>:</span>
              <Segment label="Minutes" value={clock().minutes} max={59} editable={props.editable} onCommit={value => setTimePart("minutes", value)} />
              <span class={styles.Colon}>:</span>
              <Segment label="Seconds" value={clock().seconds} max={59} editable={props.editable} onCommit={value => setTimePart("seconds", value)} />
              <Show when={fractionDigits > 0}>
                <span class={styles.Colon}>.</span>
                <input
                  class={styles.Segment}
                  style={{ width: `${fractionDigits + 1.6}ch` }}
                  value={clock().fraction.padEnd(fractionDigits, "0")}
                  aria-label="Fraction of a second"
                  ref={element => tooltip(element, () => "Fraction of a second")}
                  inputMode="numeric"
                  readOnly={!props.editable}
                  onFocus={event => event.currentTarget.select()}
                  onChange={event => setFraction(event.currentTarget.value)}
                />
              </Show>
            </div>
          </Show>
          <Show when={facts().length > 0}>
            <Facts items={facts()} />
          </Show>
        </div>
      </div>
    </div>
  );
}

export default TemporalView;
