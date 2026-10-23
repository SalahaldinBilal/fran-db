export type TemporalKind = "date" | "time" | "datetime";

export type DateParts = { year: number, month: number, day: number };

export type TimeParts = {
  negative: boolean,
  hours: number,
  minutes: number,
  seconds: number,
  fraction: string,
};

// `zone` is a UTC offset such as "+02" or "-05:30", only for types that store one.
export type TemporalValue = { date: DateParts | null, time: TimeParts | null, zone?: string | null };

const DATE_PATTERN = /^(\d{4})-(\d{1,2})-(\d{1,2})$/;
const TIME_PATTERN = /^(-)?(\d{1,3}):(\d{1,2}):(\d{1,2})(?:\.(\d{1,6}))?$/;
const ZONE_PATTERN = /(z|[+-]\d{2}(?::?\d{2}){0,2})$/i;
const MAX_TIME_HOURS = 838;
const DAY_MS = 86_400_000;
const UTC_ZONE = "+00";

export function temporalKind(typeName: string): TemporalKind {
  const base = typeName.toUpperCase();
  if (base === "DATE") return "date";
  if (base === "TIME" || base === "TIMETZ") return "time";
  return "datetime";
}

export function isZonedType(typeName: string): boolean {
  const base = typeName.toUpperCase();
  return base === "TIMESTAMPTZ" || base === "TIMETZ";
}

export function utcDate(date: DateParts): Date {
  const result = new Date(0);
  result.setUTCFullYear(date.year, date.month - 1, date.day);
  return result;
}

export function daysInMonth(year: number, month: number): number {
  return utcDate({ year, month: month + 1, day: 0 }).getUTCDate();
}

export function isZeroDate(date: DateParts): boolean {
  return date.year === 0 && date.month === 0 && date.day === 0;
}

function parseDate(text: string): DateParts | null {
  const match = DATE_PATTERN.exec(text);
  if (!match) return null;
  const [year, month, day] = match.slice(1, 4).map(Number);
  const date = { year, month, day };
  if (isZeroDate(date)) return date;
  if (month < 1 || month > 12 || day < 1 || day > daysInMonth(year, month)) return null;
  return date;
}

function parseTime(text: string, maxHours: number, allowNegative: boolean): TimeParts | null {
  const match = TIME_PATTERN.exec(text);
  if (!match) return null;
  const negative = match[1] === "-";
  const [hours, minutes, seconds] = match.slice(2, 5).map(Number);
  if ((negative && !allowNegative) || hours > maxHours || minutes > 59 || seconds > 59) return null;
  return { negative, hours, minutes, seconds, fraction: match[5] ?? "" };
}

function splitZone(clock: string): [string, string | null] {
  const match = ZONE_PATTERN.exec(clock);
  if (!match) return [clock, null];
  const zone = match[1].toLowerCase() === "z" ? UTC_ZONE : match[1];
  return [clock.slice(0, match.index).trimEnd(), zone];
}

export function parseTemporal(kind: TemporalKind, text: string, zoned = false): TemporalValue | null {
  const trimmed = text.trim();
  if (kind === "time") {
    const [clock, zone] = zoned ? splitZone(trimmed) : [trimmed, null];
    const time = parseTime(clock, MAX_TIME_HOURS, !zoned);
    return time ? { date: null, time, zone } : null;
  }

  const parts = trimmed.split(/[ T]/);
  if (parts.length > 2) return null;
  const date = parseDate(parts[0]);
  if (!date) return null;
  if (kind === "date") return parts.length === 1 ? { date, time: null } : null;
  if (parts.length === 1) return { date, time: midnight() };

  const [clock, zone] = zoned ? splitZone(parts[1]) : [parts[1], null];
  const time = parseTime(clock, 23, false);
  return time ? { date, time, zone } : null;
}

export type TemporalReading = { value: TemporalValue, note: string | null };

const EPOCH_PATTERN = /^(\d{9,17})(?:\.(\d{1,9}))?$/;
const LOOSE_PATTERN = /^(\d{4})[-/.](\d{1,2})[-/.](\d{1,2})(?:(?:\s+|T)(\d{1,2}):(\d{1,2})(?::(\d{1,2})(?:[.,](\d{1,9}))?)?)?\s*(z|utc|gmt|[+-]\d{2}(?::?\d{2})?)?$/i;
const TWELVE_HOUR_PATTERN = /^(\d{1,2})(?::(\d{2}))?(?::(\d{2}))?\s*([ap])\.?\s*m\.?$/i;
const SHORT_TIME_PATTERN = /^(-)?(\d{1,3}):(\d{1,2})$/;
const EXPLICIT_ZONE_PATTERN = /(\bgmt\b|\butc\b|z$|[+-]\d{2}:?\d{2}$)/i;

// An exact instant, written out in UTC.
function instantParts(ms: number, fraction: string): TemporalValue | null {
  const instant = new Date(ms);
  if (Number.isNaN(instant.getTime())) return null;
  return {
    date: { year: instant.getUTCFullYear(), month: instant.getUTCMonth() + 1, day: instant.getUTCDate() },
    time: { negative: false, hours: instant.getUTCHours(), minutes: instant.getUTCMinutes(), seconds: instant.getUTCSeconds(), fraction },
    zone: UTC_ZONE,
  };
}

function readEpoch(text: string): TemporalReading | null {
  const match = EPOCH_PATTERN.exec(text);
  if (!match) return null;
  const [, digits, decimals = ""] = match;

  if (digits.length <= 11) {
    const fraction = decimals.slice(0, 6);
    return wrap(instantParts(Number(digits) * 1000, fraction), "Unix time in seconds, as UTC");
  }
  if (decimals) return null;
  const unit = digits.length <= 14 ? 3 : 6;
  const seconds = Number(digits.slice(0, -unit));
  return wrap(instantParts(seconds * 1000, digits.slice(-unit)), `Unix time in ${unit === 3 ? "milliseconds" : "microseconds"}, as UTC`);
}

function zoneOffsetMinutes(zone: string): number {
  const match = /^([+-])(\d{2}):?(\d{2})?/.exec(zone);
  if (!match) return 0;
  const minutes = Number(match[2]) * 60 + Number(match[3] ?? 0);
  return match[1] === "-" ? -minutes : minutes;
}

function readLooseDateTime(text: string): TemporalReading | null {
  const match = LOOSE_PATTERN.exec(text);
  if (!match) return null;
  const [year, month, day, hours = 0, minutes = 0, seconds = 0] = match.slice(1, 7).map(part => (part === undefined ? undefined : Number(part)));
  const fraction = (match[7] ?? "").slice(0, 6);
  const zone = match[8];

  if (month! < 1 || month! > 12 || day! < 1 || day! > daysInMonth(year!, month!)) return null;
  if (hours > 23 || minutes > 59 || seconds > 59) return null;

  if (zone) {
    const date = utcDate({ year: year!, month: month!, day: day! });
    date.setUTCHours(hours, minutes, seconds);
    const offset = zoneOffsetMinutes(zone);
    return wrap(instantParts(date.getTime() - offset * 60_000, fraction), offset === 0 ? null : "converted to UTC");
  }

  return {
    value: { date: { year: year!, month: month!, day: day! }, time: { negative: false, hours, minutes, seconds, fraction } },
    note: null,
  };
}

function readNaturalDateTime(text: string): TemporalReading | null {
  if (!/\d{4}/.test(text) || !/[a-z/]/i.test(text)) return null;
  const ms = Date.parse(text);
  if (Number.isNaN(ms)) return null;
  if (EXPLICIT_ZONE_PATTERN.test(text)) return wrap(instantParts(ms, ""), "converted to UTC");

  const local = new Date(ms);
  return {
    value: {
      date: { year: local.getFullYear(), month: local.getMonth() + 1, day: local.getDate() },
      time: { negative: false, hours: local.getHours(), minutes: local.getMinutes(), seconds: local.getSeconds(), fraction: "" },
    },
    note: null,
  };
}

function readLooseTime(text: string): TemporalReading | null {
  const short = SHORT_TIME_PATTERN.exec(text);
  if (short) {
    const hours = Number(short[2]);
    const minutes = Number(short[3]);
    if (hours > MAX_TIME_HOURS || minutes > 59) return null;
    return { value: { date: null, time: { negative: short[1] === "-", hours, minutes, seconds: 0, fraction: "" } }, note: null };
  }

  const twelve = TWELVE_HOUR_PATTERN.exec(text);
  if (twelve) {
    const hour = Number(twelve[1]);
    const minutes = Number(twelve[2] ?? 0);
    const seconds = Number(twelve[3] ?? 0);
    if (hour < 1 || hour > 12 || minutes > 59 || seconds > 59) return null;
    const hours = (hour % 12) + (twelve[4].toLowerCase() === "p" ? 12 : 0);
    return { value: { date: null, time: { negative: false, hours, minutes, seconds, fraction: "" } }, note: null };
  }

  return null;
}

function wrap(value: TemporalValue | null, note: string | null): TemporalReading | null {
  return value ? { value, note } : null;
}

// Zoned types keep the offset, the others take the UTC time without one.
function fitZone(reading: TemporalReading, kind: TemporalKind, zoned: boolean): TemporalReading {
  const keepZone = zoned && kind !== "date";
  return { ...reading, value: { ...reading.value, zone: keepZone ? reading.value.zone ?? null : null } };
}

// Strict server text first, then common pasted forms. Zoned input and Unix time come out as UTC.
export function readTemporal(kind: TemporalKind, text: string, zoned = false): TemporalReading | null {
  const trimmed = text.trim();
  if (!trimmed) return null;

  const strict = parseTemporal(kind, trimmed, zoned);
  if (strict) return { value: strict, note: null };

  if (kind === "time") {
    const time = readLooseTime(trimmed);
    if (time) return time;
  }

  const reading = readEpoch(trimmed) ?? readLooseDateTime(trimmed) ?? readNaturalDateTime(trimmed);
  if (!reading) return null;

  const notes = reading.note ? [reading.note] : [];
  if (kind === "date") {
    const time = reading.value.time;
    if (time && (time.hours || time.minutes || time.seconds)) notes.push("time left out");
    return { value: { date: reading.value.date, time: null }, note: notes.join(", ") || null };
  }
  if (kind === "time") {
    notes.push("date left out");
    return fitZone({ value: { date: null, time: reading.value.time, zone: reading.value.zone }, note: notes.join(", ") }, kind, zoned);
  }
  return fitZone({ value: reading.value, note: notes.join(", ") || null }, kind, zoned);
}

// The text to store for any accepted input, with the fraction cut or padded to the column's digits when known.
export function canonicalTemporal(kind: TemporalKind, text: string, fractionDigits: number | null, zoned = false): string | null {
  const reading = readTemporal(kind, text, zoned);
  if (!reading) return null;
  const time = reading.value.time;
  if (time && fractionDigits !== null) {
    const fitted = time.fraction.padEnd(fractionDigits, "0").slice(0, fractionDigits);
    return formatTemporal(kind, { ...reading.value, time: { ...time, fraction: fitted } });
  }
  if (time && /^0*$/.test(time.fraction)) {
    return formatTemporal(kind, { ...reading.value, time: { ...time, fraction: "" } });
  }
  return formatTemporal(kind, reading.value);
}

export function fractionDigitsOf(kind: TemporalKind, value: string | null, zoned = false): number | null {
  if (value === null) return null;
  return parseTemporal(kind, value, zoned)?.time?.fraction.length ?? null;
}

export function midnight(fractionDigits = 0): TimeParts {
  return { negative: false, hours: 0, minutes: 0, seconds: 0, fraction: "0".repeat(fractionDigits) };
}

const pad = (value: number, width = 2): string => String(value).padStart(width, "0");

export function formatDate(date: DateParts): string {
  return `${pad(date.year, 4)}-${pad(date.month)}-${pad(date.day)}`;
}

export function formatTime(time: TimeParts): string {
  const clock = `${time.negative ? "-" : ""}${pad(time.hours)}:${pad(time.minutes)}:${pad(time.seconds)}`;
  return time.fraction ? `${clock}.${time.fraction}` : clock;
}

export function formatTemporal(kind: TemporalKind, value: TemporalValue): string {
  const date = value.date ? formatDate(value.date) : "";
  const time = value.time ? formatTime(value.time) : "";
  const zone = value.zone ?? "";
  if (kind === "date") return date;
  if (kind === "time") return `${time}${zone}`;
  return `${date} ${time || formatTime(midnight())}${zone}`;
}

function localOffset(now: Date): string {
  const minutes = -now.getTimezoneOffset();
  const sign = minutes < 0 ? "-" : "+";
  const absolute = Math.abs(minutes);
  const hours = pad(Math.floor(absolute / 60));
  return absolute % 60 === 0 ? `${sign}${hours}` : `${sign}${hours}:${pad(absolute % 60)}`;
}

export function currentTemporal(fractionDigits: number, zoned = false): TemporalValue {
  const now = new Date();
  const fraction = String(now.getMilliseconds()).padStart(3, "0").padEnd(6, "0").slice(0, fractionDigits);
  return {
    date: { year: now.getFullYear(), month: now.getMonth() + 1, day: now.getDate() },
    time: { negative: false, hours: now.getHours(), minutes: now.getMinutes(), seconds: now.getSeconds(), fraction },
    zone: zoned ? localOffset(now) : null,
  };
}

export function today(): DateParts {
  const now = new Date();
  return { year: now.getFullYear(), month: now.getMonth() + 1, day: now.getDate() };
}

export function sameDate(a: DateParts | null, b: DateParts | null): boolean {
  return a !== null && b !== null && a.year === b.year && a.month === b.month && a.day === b.day;
}

// 42 days starting on the Monday on or before the 1st, so every month fits in six rows.
export function monthGrid(year: number, month: number): DateParts[] {
  const first = utcDate({ year, month, day: 1 });
  const lead = (first.getUTCDay() + 6) % 7;
  const cells: DateParts[] = [];
  for (let index = 0; index < 42; index++) {
    const day = new Date(first.getTime() + (index - lead) * DAY_MS);
    cells.push({ year: day.getUTCFullYear(), month: day.getUTCMonth() + 1, day: day.getUTCDate() });
  }
  return cells;
}

export function shiftMonth(year: number, month: number, delta: number): { year: number, month: number } {
  const index = year * 12 + (month - 1) + delta;
  return { year: Math.floor(index / 12), month: (index % 12) + 1 };
}

// Values without a zone are read as local time, values with one as that exact instant.
function instantOf(date: DateParts, time: TimeParts | null, zone: string | null): Date {
  if (zone) {
    const utc = utcDate(date);
    if (time) utc.setUTCHours(time.hours, time.minutes, time.seconds, 0);
    return new Date(utc.getTime() - zoneOffsetMinutes(zone) * 60_000);
  }
  const result = new Date(2000, 0, 1);
  result.setFullYear(date.year, date.month - 1, date.day);
  if (time) result.setHours(time.hours, time.minutes, time.seconds, 0);
  return result;
}

const RELATIVE_UNITS: Array<[Intl.RelativeTimeFormatUnit, number]> = [
  ["year", 365 * DAY_MS],
  ["month", 30 * DAY_MS],
  ["week", 7 * DAY_MS],
  ["day", DAY_MS],
  ["hour", 3_600_000],
  ["minute", 60_000],
  ["second", 1000],
];

const relativeFormat = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });

export function relativeTo(date: DateParts, time: TimeParts | null, zone: string | null = null, now = new Date()): string {
  if (!time) {
    const days = Math.round((utcDate(date).getTime() - utcDate(today()).getTime()) / DAY_MS);
    if (Math.abs(days) < 7) return relativeFormat.format(days, "day");
  }

  const delta = instantOf(date, time, zone).getTime() - now.getTime();
  for (const [unit, size] of RELATIVE_UNITS) {
    if (Math.abs(delta) >= size || unit === "second") return relativeFormat.format(Math.round(delta / size), unit);
  }
  return "";
}

// Values without a zone count as UTC here.
export function unixSeconds(date: DateParts, time: TimeParts | null, zone: string | null = null): string {
  const seconds = Math.floor(utcDate(date).getTime() / 1000)
    + (time ? time.hours * 3600 + time.minutes * 60 + time.seconds : 0)
    - (zone ? zoneOffsetMinutes(zone) * 60 : 0);
  return time?.fraction && Number(time.fraction) > 0 ? `${seconds}.${time.fraction}` : String(seconds);
}

export function isoWeek(date: DateParts): { week: number, year: number } {
  const thursday = utcDate(date);
  thursday.setUTCDate(thursday.getUTCDate() - ((thursday.getUTCDay() + 6) % 7) + 3);
  const weekYear = thursday.getUTCFullYear();
  const firstThursday = utcDate({ year: weekYear, month: 1, day: 4 });
  firstThursday.setUTCDate(firstThursday.getUTCDate() - ((firstThursday.getUTCDay() + 6) % 7) + 3);
  return { week: 1 + Math.round((thursday.getTime() - firstThursday.getTime()) / (7 * DAY_MS)), year: weekYear };
}

export function dayOfYear(date: DateParts): number {
  return Math.round((utcDate(date).getTime() - utcDate({ year: date.year, month: 1, day: 1 }).getTime()) / DAY_MS) + 1;
}

export function describeDuration(time: TimeParts): string {
  const parts: string[] = [];
  if (time.hours) parts.push(`${time.hours} h`);
  if (time.minutes) parts.push(`${time.minutes} min`);
  if (time.seconds || time.fraction.replace(/0/g, "") || parts.length === 0) {
    const fraction = time.fraction.replace(/0+$/, "");
    parts.push(`${time.seconds}${fraction ? `.${fraction}` : ""} s`);
  }
  return `${time.negative ? "minus " : ""}${parts.join(" ")}`;
}

export function totalSeconds(time: TimeParts): string {
  const total = time.hours * 3600 + time.minutes * 60 + time.seconds;
  const fraction = time.fraction.replace(/0+$/, "");
  return `${time.negative ? "-" : ""}${total.toLocaleString()}${fraction ? `.${fraction}` : ""}`;
}
