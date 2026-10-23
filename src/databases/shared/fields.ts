export const IS_WINDOWS = navigator.userAgent.includes("Windows");

export function toPort(value: string): number {
  const port = Number.parseInt(value, 10);
  return Number.isNaN(port) ? 0 : Math.min(Math.max(port, 0), 65535);
}

export function toTimeout(value: string): number {
  return Math.max(1, Number.parseInt(value, 10) || 1);
}
