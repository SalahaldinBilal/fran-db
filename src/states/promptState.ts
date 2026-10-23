import { createRoot, createSignal } from "solid-js";
import { HostKeyInfo, SecretSlot } from "@core/types";

export type SecretsAnswer = { values: Record<string, string>, save: boolean };

type SecretsRequest = {
  connectionName: string,
  slots: SecretSlot[],
  canSave: boolean,
  resolve: (answer: SecretsAnswer | null) => void,
};

type HostKeyRequest = {
  key: HostKeyInfo,
  knownFingerprint: string | null,
  resolve: (trusted: boolean) => void,
};

export type ConfirmOptions = {
  title: string,
  message: string,
  confirmLabel: string,
  danger?: boolean,
};

type ConfirmRequest = ConfirmOptions & { resolve: (confirmed: boolean) => void };

export type TextPromptOptions = {
  title: string,
  label: string,
  initial: string,
  confirmLabel: string,
};

type TextRequest = TextPromptOptions & { resolve: (value: string | null) => void };

function usePromptStateInner() {
  const [secretsRequest, setSecretsRequest] = createSignal<SecretsRequest | null>(null);
  const [hostKeyRequest, setHostKeyRequest] = createSignal<HostKeyRequest | null>(null);
  const [confirmRequest, setConfirmRequest] = createSignal<ConfirmRequest | null>(null);
  const [textRequest, setTextRequest] = createSignal<TextRequest | null>(null);

  function askForText(options: TextPromptOptions): Promise<string | null> {
    return new Promise(resolve => {
      setTextRequest({
        ...options,
        resolve: value => {
          setTextRequest(null);
          resolve(value);
        },
      });
    });
  }

  function confirm(options: ConfirmOptions): Promise<boolean> {
    return new Promise(resolve => {
      setConfirmRequest({
        ...options,
        resolve: confirmed => {
          setConfirmRequest(null);
          resolve(confirmed);
        },
      });
    });
  }

  function askForSecrets(connectionName: string, slots: SecretSlot[], canSave: boolean): Promise<SecretsAnswer | null> {
    return new Promise(resolve => {
      setSecretsRequest({
        connectionName,
        slots,
        canSave,
        resolve: answer => {
          setSecretsRequest(null);
          resolve(answer);
        },
      });
    });
  }

  function askToTrustHostKey(key: HostKeyInfo, knownFingerprint: string | null): Promise<boolean> {
    return new Promise(resolve => {
      setHostKeyRequest({
        key,
        knownFingerprint,
        resolve: trusted => {
          setHostKeyRequest(null);
          resolve(trusted);
        },
      });
    });
  }

  return {
    secretsRequest,
    hostKeyRequest,
    confirmRequest,
    textRequest,
    askForSecrets,
    askToTrustHostKey,
    confirm,
    askForText,
  };
}

const usePromptState = createRoot(usePromptStateInner);
export default usePromptState;
