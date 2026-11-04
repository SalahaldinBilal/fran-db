export type ConnectionHost = { host: string, port: number | null };

export type ConnectionUrl = {
  scheme: string,
  user: string | null,
  password: string | null,
  hosts: ConnectionHost[],
  path: string | null,
  params: [string, string][],
};

export class ConnectionStringError extends Error {}

function decode(part: string): string {
  try {
    return decodeURIComponent(part);
  } catch {
    return part;
  }
}

export function toPortNumber(value: string): number {
  const port = Number(value);
  if (!Number.isInteger(port) || port < 1 || port > 65535) {
    throw new ConnectionStringError(`${value} isn't a valid port.`);
  }
  return port;
}

function parseHost(part: string): ConnectionHost {
  const bracketed = /^\[([^\]]+)\](?::(\d+))?$/.exec(part);
  if (bracketed) return { host: bracketed[1], port: bracketed[2] ? toPortNumber(bracketed[2]) : null };
  const colon = part.lastIndexOf(":");
  if (colon >= 0 && /^\d+$/.test(part.slice(colon + 1))) {
    return { host: decode(part.slice(0, colon)), port: toPortNumber(part.slice(colon + 1)) };
  }
  return { host: decode(part), port: null };
}

// The scheme without a `jdbc:` prefix or a `+driver` suffix, so `postgresql+psycopg://` reads as `postgresql`.
export function urlScheme(text: string): string | null {
  const match = /^(?:jdbc:)?([a-z][a-z0-9.-]*)(?:\+[a-z0-9.-]+)?:\/\//i.exec(text.trim());
  return match ? match[1].toLowerCase() : null;
}

export function parseUrl(text: string): ConnectionUrl | null {
  const match = /^(?:jdbc:)?([a-z][a-z0-9+.-]*):\/\/([^#]*)/i.exec(text.trim());
  if (!match) return null;

  const rest = match[2];
  const queryAt = rest.indexOf("?");
  const main = queryAt >= 0 ? rest.slice(0, queryAt) : rest;
  const query = queryAt >= 0 ? rest.slice(queryAt + 1) : "";

  const at = main.lastIndexOf("@");
  const userInfo = at >= 0 ? main.slice(0, at) : null;
  const location = at >= 0 ? main.slice(at + 1) : main;
  const slash = location.indexOf("/");
  const hostList = slash >= 0 ? location.slice(0, slash) : location;
  const path = slash >= 0 ? location.slice(slash + 1) : "";

  let user: string | null = null;
  let password: string | null = null;
  if (userInfo !== null) {
    const colon = userInfo.indexOf(":");
    user = decode(colon >= 0 ? userInfo.slice(0, colon) : userInfo) || null;
    password = colon >= 0 ? decode(userInfo.slice(colon + 1)) : null;
  }

  return {
    scheme: urlScheme(text) ?? match[1].toLowerCase(),
    user,
    password,
    hosts: hostList ? hostList.split(",").map(parseHost) : [],
    path: path ? decode(path) : null,
    params: [...new URLSearchParams(query).entries()],
  };
}

// libpq's `host=db port=5432 user='a b'` form. Null when the text isn't in that form.
export function parseKeyValues(text: string): [string, string][] | null {
  const source = text.trim();
  const pattern = /([A-Za-z_][\w]*)\s*=\s*(?:'((?:\\.|[^'\\])*)'|([^\s']*))\s*/y;
  const pairs: [string, string][] = [];
  let index = 0;
  while (index < source.length) {
    pattern.lastIndex = index;
    const match = pattern.exec(source);
    if (!match) return null;
    const value = match[2] !== undefined ? match[2].replace(/\\(.)/g, "$1") : match[3];
    pairs.push([match[1].toLowerCase(), value]);
    index = pattern.lastIndex;
  }
  return pairs.length > 0 ? pairs : null;
}

export function isTrue(value: string): boolean {
  return /^(1|true|yes|on)$/i.test(value.trim());
}

export function isFalse(value: string): boolean {
  return /^(0|false|no|off)$/i.test(value.trim());
}

export function positiveSeconds(value: string): number | null {
  const seconds = Number(value);
  return Number.isFinite(seconds) && seconds > 0 ? Math.ceil(seconds) : null;
}
