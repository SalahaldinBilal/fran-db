export function isNotNullish<T>(value: T | null | undefined): value is T {
  return value !== null && value !== undefined;
}

export function generateId(): string {
  return crypto.randomUUID();
}

const BYTE_UNITS = ["B", "KB", "MB", "GB", "TB"];

export function formatBytes(bytes: number): string {
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < BYTE_UNITS.length - 1) {
    value /= 1024;
    unit++;
  }
  return `${value < 10 && unit > 0 ? value.toFixed(1) : Math.round(value)} ${BYTE_UNITS[unit]}`;
}

const compactNumber = new Intl.NumberFormat(undefined, { notation: "compact", maximumFractionDigits: 1 });

export function formatCount(count: number): string {
  return count < 1000 ? String(count) : compactNumber.format(count);
}

export function formatDuration(ms: number): string {
  if (ms < 1) return "<1 ms";
  if (ms < 1000) return `${Math.round(ms)} ms`;
  return `${(ms / 1000).toFixed(2)} s`;
}

export function keepOrder<T>(previous: T[], next: T[]): T[] {
  const present = new Set(next);
  const kept = previous.filter(item => present.has(item));
  const known = new Set(kept);
  return [...kept, ...next.filter(item => !known.has(item))];
}

export function plural(count: number, singular: string, pluralForm = `${singular}s`): string {
  return `${count.toLocaleString()} ${count === 1 ? singular : pluralForm}`;
}
