import shared from "./ConnectionForm.module.scss";
import { Show, createMemo, createResource } from "solid-js";
import { RefreshCw } from "lucide-solid";
import { KubeOptions } from "@core/types";
import Field, { FieldRow, FieldSection } from "@core/components/Field/Field";
import Input from "@core/components/Input/Input";
import Select, { SelectItem } from "@core/components/Select/Select";
import Button from "@core/components/Button/Button";
import FilePathInput from "@core/components/FilePathInput/FilePathInput";
import { safeInvoke } from "@core/helpers/safeInvoke";
import { errorMessage } from "@core/helpers/errors";

export type KubeSectionProps = {
  kube: KubeOptions,
  description: string,
  update: (mutate: (kube: KubeOptions) => void) => void,
};

function KubeSection(props: KubeSectionProps) {
  const [contexts, { refetch }] = createResource(
    () => props.kube.kubectlPath,
    kubectlPath => safeInvoke("list_kube_contexts", { kubectlPath }),
  );

  const items = createMemo<SelectItem<string>[]>(() => {
    const loaded = contexts.latest;
    const names = loaded?.contexts ?? [];
    const items = names.map(name => ({
      value: name,
      label: name,
      hint: name === loaded?.current ? "kubectl's current context" : undefined,
    }));
    if (props.kube.context && !names.includes(props.kube.context)) {
      items.unshift({ value: props.kube.context, label: props.kube.context, hint: "not in your kubeconfig" });
    }
    return items;
  });
  const placeholder = createMemo(() => (contexts.loading ? "Reading kubeconfig…" : "Choose a context"));

  return (
    <FieldSection title="Kubernetes" description={props.description}>
      <Field label="Context" hint="Always used for this connection, whatever kubectl's current context is.">
        <div class={shared.Picker}>
          <Select
            value={props.kube.context}
            items={items()}
            placeholder={placeholder()}
            onChange={context => props.update(kube => { kube.context = context; })}
          />
          <Button icon tooltip="Reload contexts" onClick={() => void refetch()}>
            <RefreshCw size={14} />
          </Button>
        </div>
      </Field>
      <Show when={contexts.error}>
        <div class={shared.Note}>{errorMessage(contexts.error)}</div>
      </Show>
      <FieldRow>
        <Field label="Namespace" hint="Empty uses the context's namespace." grow>
          <Input
            value={props.kube.namespace}
            onValue={value => props.update(kube => { kube.namespace = value; })}
            placeholder="default"
          />
        </Field>
        <Field label="Pod or service" hint="As kubectl port-forward takes it." grow>
          <Input
            mono
            value={props.kube.resource}
            onValue={value => props.update(kube => { kube.resource = value; })}
            placeholder="svc/postgres"
          />
        </Field>
      </FieldRow>
      <Field label="kubectl" hint="Leave empty to find kubectl on PATH.">
        <FilePathInput
          value={props.kube.kubectlPath}
          onValue={value => props.update(kube => { kube.kubectlPath = value; })}
          placeholder="kubectl"
          dialogTitle="Choose the kubectl program"
        />
      </Field>
    </FieldSection>
  );
}

export default KubeSection;
