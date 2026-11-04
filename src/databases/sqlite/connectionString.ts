import { SqliteOptions } from "@core/types";
import { ParsedConnection } from "../types";

const DATABASE_FILE = /\.(db|db3|sqlite|sqlite3)$/i;

function decode(part: string): string {
  try {
    return decodeURIComponent(part);
  } catch {
    return part;
  }
}

// Reads `sqlite:path`, `sqlite:///abs/path`, `file:path?mode=ro`, `jdbc:sqlite:path` and bare database file paths.
export function parseSqliteConnectionString(text: string, current: SqliteOptions): ParsedConnection<SqliteOptions> | null {
  const trimmed = text.trim();
  const prefixed = /^(?:jdbc:)?(?:sqlite3?|file):(.*)$/i.exec(trimmed);
  let path: string;
  let query = "";

  if (prefixed) {
    let rest = prefixed[1];
    const queryAt = rest.indexOf("?");
    if (queryAt >= 0) {
      query = rest.slice(queryAt + 1);
      rest = rest.slice(0, queryAt);
    }
    if (rest.startsWith("//")) rest = rest.slice(2);
    if (/^\/[a-z]:[\\/]/i.test(rest)) rest = rest.slice(1);
    path = decode(rest);
  } else if (DATABASE_FILE.test(trimmed) && !trimmed.includes("://")) {
    path = trimmed;
  } else {
    return null;
  }

  const options: SqliteOptions = { ...current, path };
  const filled = new Set(["file"]);
  const ignored: string[] = [];

  for (const [key, value] of new URLSearchParams(query).entries()) {
    if (key.toLowerCase() !== "mode") {
      ignored.push(key);
      continue;
    }
    switch (value.toLowerCase()) {
      case "ro":
        options.readOnly = true;
        filled.add("read only");
        break;
      case "rw":
        options.readOnly = false;
        options.createIfMissing = false;
        filled.add("read only");
        break;
      case "rwc":
        options.readOnly = false;
        options.createIfMissing = true;
        filled.add("create if missing");
        break;
      default:
        ignored.push(`mode=${value}`);
    }
  }

  return { options, password: null, filled: [...filled], ignored };
}
