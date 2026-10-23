import styles from "./Home.module.scss";
import { For, Match, Show, Switch, createMemo, createSignal } from "solid-js";
import { CircleAlert, Plus, Search, ShieldAlert } from "lucide-solid";
import useConnectionsState from "@core/states/connectionsState";
import useVaultState from "@core/states/vaultState";
import usePromptState from "@core/states/promptState";
import { withDatabaseModule } from "@core/databases";
import { errorMessage } from "@core/helpers/errors";
import { ConnectionProfile } from "@core/types";
import Button from "@core/components/Button/Button";
import Input from "@core/components/Input/Input";
import EmptyState from "@core/components/EmptyState/EmptyState";
import Spinner from "@core/components/Spinner/Spinner";
import ConnectionCard from "./ConnectionCard/ConnectionCard";
import ConnectionEditor from "./ConnectionEditor/ConnectionEditor";

type EditorState = { open: false } | { open: true, profile: ConnectionProfile | null };

function Home() {
  const connections = useConnectionsState;
  const vault = useVaultState;
  const { confirm } = usePromptState;
  const [query, setQuery] = createSignal("");
  const [editor, setEditor] = createSignal<EditorState>({ open: false });
  const editingProfile = createMemo(() => {
    const state = editor();
    return state.open ? state.profile : null;
  });

  const filtered = createMemo(() => {
    const needle = query().trim().toLowerCase();
    if (!needle) return connections.connections;

    return connections.connections.filter(profile => {
      const target = withDatabaseModule(profile, (module, options) => module.describeTarget(options));
      return profile.name.toLowerCase().includes(needle) || target.toLowerCase().includes(needle);
    });
  });

  async function resetVault() {
    const confirmed = await confirm({
      title: "Reset the vault?",
      message: "Every saved password and passphrase will be deleted. Connections stay, you'll be asked for credentials again.",
      confirmLabel: "Reset vault",
      danger: true,
    });
    if (confirmed) await vault.reset();
  }

  return (
    <div class={styles.Home}>
      <header class={styles.Header}>
        <h1>Connections</h1>
        <Show when={connections.connections.length > 0}>
          <span class={styles.Count}>{connections.connections.length}</span>
        </Show>
        <div class={styles.Spacer} />
        <Input
          class={styles.Search}
          type="search"
          value={query()}
          onValue={setQuery}
          placeholder="Filter"
          before={<Search size={14} />}
        />
        <Button variant="primary" onClick={() => setEditor({ open: true, profile: null })}>
          <Plus size={14} />
          New connection
        </Button>
      </header>

      <Show when={vault.status() && !vault.status()!.available}>
        <div class={styles.Banner}>
          <ShieldAlert size={16} />
          <span>Saved passwords are unavailable. {vault.status()?.message}</span>
          <Button size="sm" variant="subtle" onClick={resetVault}>Reset vault</Button>
        </div>
      </Show>

      <div class={styles.Body}>
        <Switch>
          <Match when={!connections.loaded()}>
            <div class={styles.Center}><Spinner size={18} /></div>
          </Match>
          <Match when={connections.loadError()}>
            <EmptyState
              tone="danger"
              icon={CircleAlert}
              title="Connections couldn't be loaded"
              description={errorMessage(connections.loadError())}
              actions={<Button variant="subtle" onClick={connections.load}>Try again</Button>}
            />
          </Match>
          <Match when={connections.connections.length === 0}>
            <EmptyState
              title="No connections yet"
              description="Passwords you save are kept in an encrypted vault on this machine."
              actions={
                <Button variant="primary" onClick={() => setEditor({ open: true, profile: null })}>
                  <Plus size={14} />
                  New connection
                </Button>
              }
            />
          </Match>
          <Match when={filtered().length === 0}>
            <EmptyState title="Nothing matches" description={`No connection matches "${query()}".`} />
          </Match>
          <Match when={true}>
            <div class={styles.Grid}>
              <For each={filtered()}>
                {profile => (
                  <ConnectionCard profile={profile} onEdit={() => setEditor({ open: true, profile })} />
                )}
              </For>
            </div>
          </Match>
        </Switch>
      </div>

      <ConnectionEditor
        show={editor().open}
        profile={editingProfile()}
        onClose={() => setEditor({ open: false })}
      />
    </div>
  );
}

export default Home;
