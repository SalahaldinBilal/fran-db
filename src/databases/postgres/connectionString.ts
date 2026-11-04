import { PostgresOptions, PostgresSslMode } from "@core/types";
import { ParsedConnection } from "../types";
import {
  ConnectionStringError, isTrue, parseKeyValues, parseUrl, positiveSeconds, toPortNumber, urlScheme,
} from "../shared/connectionString";
import { IS_WINDOWS } from "../shared/fields";

const SCHEMES = new Set(["postgres", "postgresql"]);
const KNOWN_KEYS = new Set(["host", "hostaddr", "port", "user", "dbname", "password"]);
const DEFAULT_PORT = 5432;

const SSL_MODES: Record<string, PostgresSslMode> = {
  disable: "disable",
  allow: "prefer",
  prefer: "prefer",
  require: "require",
  "verify-ca": "verifyCa",
  "verify-full": "verifyFull",
};

// URLs become the same keyword list libpq uses, with query parameters after the parts so they win like in libpq.
function keywords(text: string): [string, string][] | null {
  const scheme = urlScheme(text);
  if (scheme !== null) {
    if (!SCHEMES.has(scheme)) return null;
    const url = parseUrl(text)!;
    const pairs: [string, string][] = [];
    if (url.hosts.length > 0) {
      pairs.push(["host", url.hosts.map(host => host.host).join(",")]);
      const ports = url.hosts.map(host => host.port ?? "");
      if (ports.some(port => port !== "")) pairs.push(["port", ports.join(",")]);
    }
    if (url.user !== null) pairs.push(["user", url.user]);
    if (url.password !== null) pairs.push(["password", url.password]);
    if (url.path !== null) pairs.push(["dbname", url.path]);
    return [...pairs, ...url.params.map(([key, value]): [string, string] => [key.toLowerCase(), value])];
  }

  const pairs = parseKeyValues(text);
  return pairs && pairs.some(([key]) => KNOWN_KEYS.has(key)) ? pairs : null;
}

function searchPathSchema(options: string): string | null {
  const match = /-c\s*search_path\s*=\s*("?)([^",\s]+)\1/i.exec(options);
  return match ? match[2] : null;
}

export function parsePostgresConnectionString(text: string, current: PostgresOptions): ParsedConnection<PostgresOptions> | null {
  const pairs = keywords(text);
  if (!pairs) return null;

  const options: PostgresOptions = JSON.parse(JSON.stringify(current));
  const filled = new Set<string>();
  const ignored: string[] = [];
  let password: string | null = null;
  let sslModeSet = false;
  let portSet = false;

  for (const [key, value] of pairs) {
    switch (key) {
      case "host":
      case "hostaddr": {
        const [first, ...others] = value.split(",");
        if (others.length > 0) ignored.push(`other hosts (${others.join(", ")})`);
        if (first.startsWith("/")) {
          if (IS_WINDOWS) throw new ConnectionStringError(`${first} is a Unix socket, which Postgres connections can't use on Windows.`);
          options.socketPath = first;
          if (options.method === "tcp") options.method = "socket";
          filled.add("socket");
        } else if (first) {
          options.host = first;
          if (options.method === "socket") options.method = "tcp";
          filled.add("host");
        }
        break;
      }
      case "port": {
        const first = value.split(",")[0];
        if (first) {
          options.port = toPortNumber(first);
          portSet = true;
          filled.add("port");
        }
        break;
      }
      case "user":
        options.user = value;
        filled.add("user");
        break;
      case "password":
        password = value;
        filled.add("password");
        break;
      case "dbname":
        options.database = value;
        filled.add("database");
        break;
      case "sslmode": {
        const mode = SSL_MODES[value.toLowerCase()];
        if (!mode) throw new ConnectionStringError(`sslmode=${value} isn't a mode Postgres knows.`);
        options.ssl.mode = mode;
        sslModeSet = true;
        filled.add("SSL mode");
        break;
      }
      case "ssl":
        if (!sslModeSet && isTrue(value)) {
          options.ssl.mode = "require";
          filled.add("SSL mode");
        }
        break;
      case "sslrootcert":
        options.ssl.caFile = value;
        filled.add("SSL files");
        break;
      case "sslcert":
        options.ssl.certFile = value;
        filled.add("SSL files");
        break;
      case "sslkey":
        options.ssl.keyFile = value;
        filled.add("SSL files");
        break;
      case "connect_timeout": {
        const seconds = positiveSeconds(value);
        if (seconds !== null) {
          options.connectTimeoutSecs = seconds;
          filled.add("connect timeout");
        }
        break;
      }
      case "options": {
        const schema = searchPathSchema(value);
        if (schema) {
          options.defaultSchema = schema;
          filled.add("default schema");
        } else {
          ignored.push("options");
        }
        break;
      }
      default:
        ignored.push(key);
    }
  }

  if (filled.has("host") && !portSet) options.port = DEFAULT_PORT;
  return { options, password, filled: [...filled], ignored };
}
