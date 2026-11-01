import styles from "./Home.module.scss";
import { For, Match, Show, Switch, createMemo, createSignal } from "solid-js";
import { unwrap } from "solid-js/store";
import { Check, CircleAlert, GripVertical, Pencil, Plus, Search, ShieldAlert, X } from "lucide-solid";
import useConnectionsState from "@core/states/connectionsState";
import useVaultState from "@core/states/vaultState";
import usePromptState from "@core/states/promptState";
import useToastState from "@core/states/toastState";
import { startReorder } from "@core/helpers/dragReorder";
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
type Preview = { groups: string[] } | { id: string, group: string, ids: string[] };

function Home() {
  const connections = useConnectionsState;
  const vault = useVaultState;
  const { confirm } = usePromptState;
  const { pushError } = useToastState;
  const [query, setQuery] = createSignal("");
  const [editing, setEditing] = createSignal(false);
  const [newGroup, setNewGroup] = createSignal("");
  const [renaming, setRenaming] = createSignal<string | null>(null);
  const [preview, setPreview] = createSignal<Preview | null>(null);
  let sectionList!: HTMLDivElement;
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
      return [profile.name, profile.group ?? "", target].some(text => text.toLowerCase().includes(needle));
    });
  });

  const sectionMap = createMemo(() => {
    const current = preview();
    const moving = current && "ids" in current ? current : null;
    const byGroup = new Map<string, ConnectionProfile[]>();
    if (editing()) {
      for (const key of ["", ...connections.groupOrder()]) byGroup.set(key, []);
    }
    let dragged: ConnectionProfile | undefined;
    for (const profile of filtered()) {
      const key = profile.group ?? "";
      if (!byGroup.has(key)) byGroup.set(key, []);
      if (profile.id === moving?.id) dragged = profile;
      else byGroup.get(key)!.push(profile);
    }
    if (moving && dragged) {
      const members = [...(byGroup.get(moving.group) ?? []), dragged];
      byGroup.set(moving.group, members.sort((a, b) => moving.ids.indexOf(a.id) - moving.ids.indexOf(b.id)));
    }
    return byGroup;
  });

  const sectionKeys = createMemo(() => {
    const current = preview();
    const order = current && "groups" in current ? current.groups : connections.groupOrder();
    function rank(key: string) {
      const index = order.indexOf(key);
      return index >= 0 ? index : order.length + (key === "" ? 1 : 0);
    }
    return [...sectionMap().keys()].sort((a, b) => rank(a) - rank(b) || a.localeCompare(b));
  });

  const canAddGroup = createMemo(() => {
    const name = newGroup().trim();
    return name !== "" && !sectionMap().has(name);
  });

  function toggleEditing() {
    setEditing(!editing());
    setQuery("");
  }

  async function saveGroups(groups: string[]) {
    try {
      await connections.reorder(connections.connections.map(profile => profile.id), groups);
    } catch (error) {
      pushError(error);
    }
  }

  function addGroup() {
    if (!canAddGroup()) return;
    const name = newGroup().trim();
    const keys = sectionKeys();
    const at = keys[keys.length - 1] === "" ? keys.length - 1 : keys.length;
    setNewGroup("");
    return saveGroups([...keys.slice(0, at), name, ...keys.slice(at)]);
  }

  async function commitRename(key: string, value: string) {
    setRenaming(null);
    const name = value.trim();
    if (!name || name === key) return;
    try {
      await connections.renameGroup(key, name);
    } catch (error) {
      pushError(error);
    }
  }

  function grabGroup(event: PointerEvent, key: string, header: HTMLElement) {
    startReorder(event, {
      id: key,
      items: () => [...sectionList.children] as HTMLElement[],
      axis: "y",
      ghost: header,
      onPlace: placement => {
        if ("zone" in placement) return;
        const groups = sectionKeys().filter(other => other !== key);
        groups.splice(groups.indexOf(placement.target) + (placement.after ? 1 : 0), 0, key);
        setPreview({ groups });
      },
      onEnd: commitOrder,
    });
  }

  function grabCard(event: PointerEvent, id: string) {
    startReorder(event, {
      id,
      items: () => [...sectionList.querySelectorAll<HTMLElement>(`.${styles.Grid} > [data-id]`)],
      zones: () => [...sectionList.children] as HTMLElement[],
      axis: "x",
      onPlace: placement => {
        const map = sectionMap();
        const group = "zone" in placement
          ? placement.zone
          : [...map.keys()].find(key => map.get(key)!.some(profile => profile.id === placement.target))!;
        const ids = (map.get(group) ?? []).map(profile => profile.id).filter(other => other !== id);
        const at = "zone" in placement ? ids.length : ids.indexOf(placement.target) + (placement.after ? 1 : 0);
        ids.splice(at, 0, id);
        setPreview({ id, group, ids });
      },
      onEnd: commitOrder,
    });
  }

  async function commitOrder() {
    const current = preview();
    if (!current) return;
    try {
      if ("ids" in current) {
        const profile = connections.find(current.id);
        if (profile && (profile.group ?? "") !== current.group) {
          await connections.save({ ...unwrap(profile), group: current.group || null });
        }
      }
      const groups = sectionKeys();
      const ids = groups.flatMap(key => (sectionMap().get(key) ?? []).map(profile => profile.id));
      const saving = connections.reorder(ids, groups);
      setPreview(null);
      await saving;
    } catch (error) {
      setPreview(null);
      pushError(error);
    }
  }

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
          disabled={editing()}
          before={<Search size={14} />}
        />
        <Show when={connections.connections.length > 0}>
          <Button variant="subtle" active={editing()} onClick={toggleEditing}>
            <Show when={editing()} fallback={<Pencil size={14} />}><Check size={14} /></Show>
            {editing() ? "Done" : "Edit groups"}
          </Button>
        </Show>
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
            <div ref={sectionList}>
              <For each={sectionKeys()}>
                {key => {
                  const profiles = createMemo(() => sectionMap().get(key) ?? []);
                  let header!: HTMLElement;
                  return (
                    <section class={styles.Section} data-id={key}>
                      <Show when={key !== "" || sectionKeys().length > 1}>
                        <header ref={header} class={styles.SectionHeader}>
                          <Show when={editing()}>
                            <span class={styles.Grip} onPointerDown={event => grabGroup(event, key, header)}>
                              <GripVertical size={14} />
                            </span>
                          </Show>
                          <Show
                            when={renaming() === key}
                            fallback={
                              <h2
                                class={styles.SectionTitle}
                                classList={{ [styles.Renamable]: editing() && key !== "" }}
                                onClick={() => {
                                  if (editing() && key !== "") setRenaming(key);
                                }}
                              >
                                {key || "Ungrouped"}
                              </h2>
                            }
                          >
                            <input
                              class={styles.RenameInput}
                              value={key}
                              size={Math.max(8, key.length + 2)}
                              ref={input => queueMicrotask(() => {
                                input.focus();
                                input.select();
                              })}
                              onKeyDown={event => {
                                if (event.key === "Enter") commitRename(key, event.currentTarget.value);
                                if (event.key === "Escape") setRenaming(null);
                              }}
                              onBlur={event => {
                                if (renaming() === key) commitRename(key, event.currentTarget.value);
                              }}
                            />
                          </Show>
                          <Show when={editing() && key !== "" && profiles().length === 0}>
                            <Button
                              icon
                              size="sm"
                              class={styles.RemoveGroup}
                              tooltip="Remove group"
                              onClick={() => saveGroups(sectionKeys().filter(other => other !== key))}
                            >
                              <X size={14} />
                            </Button>
                          </Show>
                        </header>
                      </Show>
                      <div class={styles.Grid} classList={{ [styles.Empty]: profiles().length === 0 }}>
                        <Show when={profiles().length === 0}>
                          <span>Drop connections here</span>
                        </Show>
                        <For each={profiles()}>
                          {profile => {
                            const onGrab = createMemo(() => (editing() ? (event: PointerEvent) => grabCard(event, profile.id) : undefined));
                            return (
                              <div data-id={profile.id}>
                                <ConnectionCard profile={profile} onEdit={() => setEditor({ open: true, profile })} onGrab={onGrab()} />
                              </div>
                            );
                          }}
                        </For>
                      </div>
                    </section>
                  );
                }}
              </For>
            </div>
            <Show when={editing()}>
              <div class={styles.AddGroup}>
                <Input
                  class={styles.Search}
                  value={newGroup()}
                  onValue={setNewGroup}
                  placeholder="New group"
                  onKeyDown={event => {
                    if (event.key === "Enter") addGroup();
                  }}
                />
                <Button variant="subtle" disabled={!canAddGroup()} onClick={addGroup}>
                  <Plus size={14} />
                  Add group
                </Button>
              </div>
            </Show>
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
