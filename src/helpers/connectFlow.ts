import { safeInvoke } from "./safeInvoke";
import { isAppError } from "./errors";
import usePromptState from "@core/states/promptState";

export type ConnectTarget = {
  connectionId: string | null,
  connectionName: string,
};

// Retries while the backend asks for credentials or a host key decision. Null means the user cancelled.
export async function withConnectPrompts<T>(
  target: ConnectTarget,
  attempt: (secrets: Record<string, string>) => Promise<T>,
  initialSecrets: Record<string, string> = {},
): Promise<T | null> {
  const { askForSecrets, askToTrustHostKey } = usePromptState;
  const secrets = { ...initialSecrets };

  for (;;) {
    try {
      return await attempt(secrets);
    } catch (error) {
      if (!isAppError(error)) throw error;
      const detail = error.detail;

      if (detail.kind === "secretsRequired") {
        const answer = await askForSecrets(target.connectionName, detail.slots, target.connectionId !== null);
        if (!answer) return null;

        Object.assign(secrets, answer.values);
        if (answer.save && target.connectionId) {
          for (const [slot, value] of Object.entries(answer.values)) {
            await safeInvoke("set_connection_secret", { connectionId: target.connectionId, slot, value });
          }
        }
        continue;
      }

      if (detail.kind === "unknownHostKey" || detail.kind === "hostKeyChanged") {
        const knownFingerprint = detail.kind === "hostKeyChanged" ? detail.knownFingerprint : null;
        const trusted = await askToTrustHostKey(detail.key, knownFingerprint);
        if (!trusted) return null;

        await safeInvoke("trust_host_key", { key: detail.key });
        continue;
      }

      throw error;
    }
  }
}
