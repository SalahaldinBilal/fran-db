export type DatabaseKind = "mysql" | "postgres";

export type SecretSlot = {
  id: string,
  label: string,
};

export type HostKeyInfo = {
  host: string,
  port: number,
  algorithm: string,
  fingerprint: string,
};

export type SshAuthMethod = "password" | "publicKey";

export type SshOptions = {
  host: string,
  port: number,
  user: string,
  auth: SshAuthMethod,
  keyFile: string,
};

export type MysqlConnectMethod = "tcp" | "socket" | "sshTunnel";

export type SslMode = "disabled" | "preferred" | "required" | "verifyCa" | "verifyIdentity";

export type MysqlSslOptions = SslFiles & {
  mode: SslMode,
};

export type MysqlOptions = {
  method: MysqlConnectMethod,
  host: string,
  port: number,
  socketPath: string,
  user: string,
  defaultSchema: string,
  ssh: SshOptions,
  ssl: MysqlSslOptions,
  compression: boolean,
  cleartextAuth: boolean,
  connectTimeoutSecs: number,
  initSql: string,
};

export type PostgresConnectMethod = "tcp" | "socket" | "sshTunnel";

export type PostgresSslMode = "disable" | "prefer" | "require" | "verifyCa" | "verifyFull";

export type SslFiles = {
  caFile: string,
  certFile: string,
  keyFile: string,
};

export type PostgresSslOptions = SslFiles & {
  mode: PostgresSslMode,
};

export type PostgresOptions = {
  method: PostgresConnectMethod,
  host: string,
  port: number,
  socketPath: string,
  user: string,
  database: string,
  defaultSchema: string,
  ssh: SshOptions,
  ssl: PostgresSslOptions,
  connectTimeoutSecs: number,
  initSql: string,
};

export type OptionsByKind = {
  mysql: MysqlOptions,
  postgres: PostgresOptions,
};

export type DriverOptions = {
  [K in DatabaseKind]: { kind: K, options: OptionsByKind[K] }
}[DatabaseKind];

export type ConnectionProfile = {
  id: string,
  name: string,
  color: string | null,
} & DriverOptions;

export type VaultStatus = {
  available: boolean,
  message: string | null,
};
