// How a column's DEFAULT is checked: the kind of literal its type takes.
export type DefaultKind = "number" | "text" | "boolean" | "temporal" | "other";

export type DefaultProblem = {
  message: string,
  // A corrected default to offer, such as the value wrapped in quotes.
  fix: string | null,
};

const KEYWORDS = new Set([
  "null", "default", "true", "false", "current_timestamp", "current_date", "current_time", "localtime", "localtimestamp",
  "current_user", "session_user", "user",
]);
const NUMBER = /^[+-]?(\d+\.?\d*|\.\d+)(e[+-]?\d+)?$/i;
const FUNCTION_CALL = /^[a-z_][\w.$]*\s*\(.*\)$/is;
const STRING_PREFIX = /^([ebnx]|u&)?'/i;

export function quoteDefault(value: string): string {
  return `'${value.replaceAll("'", "''")}'`;
}

// True when `value` is one complete string literal, from its opening quote to the last character.
function isClosedString(value: string, backslashEscapes: boolean): boolean {
  const start = value.indexOf("'");
  for (let index = start + 1; index < value.length; index++) {
    const character = value[index];
    if (backslashEscapes && character === "\\") {
      index++;
      continue;
    }
    if (character !== "'") continue;
    if (value[index + 1] === "'") {
      index++;
      continue;
    }
    return index === value.length - 1;
  }
  return false;
}

export function checkDefault(kind: DefaultKind, raw: string, backslashEscapes: boolean): DefaultProblem | null {
  const value = raw.trim();
  if (!value) return null;

  const isExpression = value.startsWith("(") || value.includes("::") || FUNCTION_CALL.test(value)
    || KEYWORDS.has(value.toLowerCase());
  if (isExpression) return null;

  if (STRING_PREFIX.test(value)) {
    return isClosedString(value, backslashEscapes) ? null : { message: "The quote isn't closed.", fix: null };
  }

  switch (kind) {
    case "number":
      return NUMBER.test(value)
        ? null
        : { message: "Not a number. Wrap an expression in parentheses.", fix: null };
    case "boolean":
      return { message: "Use true or false.", fix: null };
    case "text":
      return NUMBER.test(value) ? null : { message: "Text needs quotes.", fix: quoteDefault(value) };
    case "temporal":
      return { message: "Dates and times need quotes.", fix: quoteDefault(value) };
    case "other":
      return null;
  }
}
