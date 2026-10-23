// Text columns often hold JSON (MariaDB's JSON type is LONGTEXT), so detect it from the content.
export function looksLikeJson(value: string): boolean {
  const trimmed = value.trim();
  const first = trimmed[0];
  const last = trimmed[trimmed.length - 1];
  if (!((first === "{" && last === "}") || (first === "[" && last === "]"))) return false;

  try {
    JSON.parse(trimmed);
    return true;
  } catch {
    return false;
  }
}

export function formatJson(value: string): string {
  return JSON.stringify(JSON.parse(value), null, 2);
}

export function compactJson(value: string): string {
  return JSON.stringify(JSON.parse(value));
}
