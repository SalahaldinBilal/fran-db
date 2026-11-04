import { MysqlOptions, SslMode } from "@core/types";
import { ParsedConnection } from "../types";
import {
  ConnectionStringError, isFalse, isTrue, parseUrl, positiveSeconds, toPortNumber, urlScheme,
} from "../shared/connectionString";

const SCHEMES = new Set(["mysql", "mariadb"]);
const DEFAULT_PORT = 3306;

const SSL_MODES: Record<string, SslMode> = {
  disabled: "disabled",
  preferred: "preferred",
  required: "required",
  verifyca: "verifyCa",
  verifyidentity: "verifyIdentity",
};

// Drivers spell the same option as ssl-mode, ssl_mode, sslMode or sslmode.
function normalize(key: string): string {
  return key.toLowerCase().replace(/[-_]/g, "");
}

// Go's driver writes timeouts with a unit, like 5s or 500ms.
function unitSeconds(value: string): number | null {
  const match = /^(\d+(?:\.\d+)?)(ms|s|m)$/i.exec(value.trim());
  if (!match) return null;
  const amount = Number(match[1]);
  const unit = match[2].toLowerCase();
  return positiveSeconds(String(unit === "ms" ? amount / 1000 : unit === "m" ? amount * 60 : amount));
}

export function parseMysqlConnectionString(text: string, current: MysqlOptions): ParsedConnection<MysqlOptions> | null {
  const scheme = urlScheme(text);
  if (scheme === null || !SCHEMES.has(scheme)) return null;
  const url = parseUrl(text)!;

  const options: MysqlOptions = JSON.parse(JSON.stringify(current));
  const filled = new Set<string>();
  const ignored: string[] = [];
  let password = url.password;
  let sslModeSet = false;

  function useSocket(path: string) {
    options.socketPath = path;
    options.method = "socket";
    filled.add("socket");
  }

  const [first, ...others] = url.hosts;
  if (others.length > 0) ignored.push(`other hosts (${others.map(host => host.host).join(", ")})`);
  if (first?.host.startsWith("/")) {
    useSocket(first.host);
  } else if (first?.host) {
    options.host = first.host;
    options.port = first.port ?? DEFAULT_PORT;
    if (options.method === "socket") options.method = "tcp";
    filled.add("host");
    filled.add("port");
  } else if (first?.port) {
    options.port = first.port;
    filled.add("port");
  }
  if (url.user !== null) {
    options.user = url.user;
    filled.add("user");
  }
  if (password !== null) filled.add("password");
  if (url.path !== null) {
    options.defaultSchema = url.path;
    filled.add("default schema");
  }

  for (const [key, value] of url.params) {
    switch (normalize(key)) {
      case "user":
        options.user = value;
        filled.add("user");
        break;
      case "password":
        password = value;
        filled.add("password");
        break;
      case "port":
        options.port = toPortNumber(value);
        filled.add("port");
        break;
      case "socket":
        useSocket(value);
        break;
      case "sslmode": {
        const mode = SSL_MODES[normalize(value)];
        if (!mode) throw new ConnectionStringError(`${key}=${value} isn't an SSL mode MySQL knows.`);
        options.ssl.mode = mode;
        sslModeSet = true;
        filled.add("SSL mode");
        break;
      }
      case "usessl":
      case "ssl":
        if (!sslModeSet && (isTrue(value) || isFalse(value))) {
          options.ssl.mode = isTrue(value) ? "required" : "disabled";
          filled.add("SSL mode");
        } else if (!sslModeSet) {
          ignored.push(key);
        }
        break;
      case "sslca":
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
      case "connecttimeout": {
        // JDBC's connectTimeout is in milliseconds, connect_timeout and connect-timeout are in seconds.
        const raw = key === "connectTimeout" ? String(Number(value) / 1000) : value;
        const seconds = positiveSeconds(raw);
        if (seconds !== null) {
          options.connectTimeoutSecs = seconds;
          filled.add("connect timeout");
        }
        break;
      }
      case "timeout": {
        const seconds = unitSeconds(value) ?? positiveSeconds(value);
        if (seconds !== null) {
          options.connectTimeoutSecs = seconds;
          filled.add("connect timeout");
        }
        break;
      }
      case "compress":
      case "compression":
      case "usecompression":
        options.compression = isTrue(value);
        filled.add("compression");
        break;
      default:
        ignored.push(key);
    }
  }

  return { options, password, filled: [...filled], ignored };
}
