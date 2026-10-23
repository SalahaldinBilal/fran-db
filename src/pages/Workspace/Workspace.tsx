import styles from "./Workspace.module.scss";
import { For, Show, createMemo, untrack } from "solid-js";
import { Database } from "lucide-solid";
import useWorkspaceState from "@core/states/workspaceState";
import { databaseModule } from "@core/databases";
import EmptyState from "@core/components/EmptyState/EmptyState";
import SchemaBrowser from "./SchemaBrowser/SchemaBrowser";
import DatabaseTabs from "./DatabaseTabs/DatabaseTabs";

function Workspace(props: { sessionId: string }) {
  const workspace = useWorkspaceState;
  const session = createMemo(() => workspace.store.sessions[props.sessionId]);
  const scopeNames = createMemo(() => Object.keys(session()?.scopes ?? {}));
  const namespace = databaseModule(untrack(() => workspace.store.sessions[props.sessionId].info.kind)).namespace;

  return (
    <Show when={session()}>
      {current => (
        <div class={styles.Workspace}>
          <SchemaBrowser sessionId={props.sessionId} />
          <section class={styles.Main}>
            <Show
              when={current().selectedDatabase}
              fallback={
                <EmptyState
                  icon={Database}
                  title={`Pick a ${namespace.singular}`}
                  description={`Choose a ${namespace.singular} on the left. Each ${namespace.singular} keeps its own tabs.`}
                />
              }
            >
              <For each={scopeNames()}>
                {database => (
                  <div
                    class={styles.Scope}
                    classList={{ [styles.Hidden]: current().selectedDatabase !== database }}
                  >
                    <DatabaseTabs sessionId={props.sessionId} database={database} />
                  </div>
                )}
              </For>
            </Show>
          </section>
        </div>
      )}
    </Show>
  );
}

export default Workspace;
