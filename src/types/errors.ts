import { HostKeyInfo, SecretSlot } from "./connection";

export type ErrorDetail =
  | { kind: "general" }
  | { kind: "notFound" }
  | { kind: "vault" }
  | { kind: "ssh" }
  | { kind: "database", code: number | null, sqlState: string | null }
  | { kind: "secretsRequired", slots: SecretSlot[] }
  | { kind: "unknownHostKey", key: HostKeyInfo }
  | { kind: "hostKeyChanged", key: HostKeyInfo, knownFingerprint: string };

export type AppError = {
  message: string,
  detail: ErrorDetail,
};
