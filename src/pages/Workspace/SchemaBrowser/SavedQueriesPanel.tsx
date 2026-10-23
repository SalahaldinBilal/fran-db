import styles from "./SchemaBrowser.module.scss";
import { For, Show, createMemo, createSignal } from "solid-js";
import { Pencil, Plus, Search, SquarePen, TextCursorInput, Trash2 } from "lucide-solid";
import useWorkspaceState from "@core/states/workspaceState";
import useSavedQueriesState from "@core/states/savedQueriesState";
import useContextMenuState from "@core/states/contextMenuState";
import usePromptState from "@core/states/promptState";
import useToastState from "@core/states/toastState";
import { SavedQuery } from "@core/types";
import Input from "@core/components/Input/Input";
import Button from "@core/components/Button/Button";
import EmptyState from "@core/components/EmptyState/EmptyState";
import SavedQueryEditor, { SavedQueryEditorState } from "./SavedQueryEditor";

function SavedQueriesPanel(props: { sessionId: string }) {
  const workspace = useWorkspaceState;
  const saved = useSavedQueriesState;
  const { openContextMenu } = useContextMenuState;
  const { askForText, confirm } = usePromptState;
  const { pushError } = useToastState;
  const [filter, setFilter] = createSignal("");
  const [editing, setEditing] = createSignal<SavedQueryEditorState>(null);

  const session = createMemo(() => workspace.store.sessions[props.sessionId]);

  const matching = createMemo(() => {
    const needle = filter().trim().toLowerCase();
    const sorted = [...saved.queries].sort((a, b) => a.name.localeCompare(b.name));
    if (!needle) return sorted;
    return sorted.filter(query => query.name.toLowerCase().includes(needle) || query.sql.toLowerCase().includes(needle));
  });

  const ownQueries = createMemo(() => matching().filter(query => query.connectionId === session()?.profile.id));
  const otherQueries = createMemo(() => matching().filter(query => query.connectionId !== session()?.profile.id));

  async function rename(query: SavedQuery) {
    const name = await askForText({ title: "Rename query", label: "Name", initial: query.name, confirmLabel: "Rename" });
    if (!name) return;
    try {
      await saved.rename(query.id, name);
    } catch (error) {
      pushError(error);
    }
  }

  async function remove(query: SavedQuery) {
    const confirmed = await confirm({
      title: `Delete ${query.name}?`,
      message: "The saved query will be removed. Tabs opened from it keep their text.",
      confirmLabel: "Delete",
      danger: true,
    });
    if (!confirmed) return;
    try {
      await saved.remove(query.id);
    } catch (error) {
      pushError(error);
    }
  }

  function QueryRow(rowProps: { query: SavedQuery }) {
    return (
      <div
        class={styles.QueryRow}
        title={rowProps.query.sql}
        onDblClick={() => workspace.openSavedQuery(props.sessionId, rowProps.query)}
        onContextMenu={event => openContextMenu(event, [
          { label: "Open in new tab", icon: SquarePen, onSelect: () => workspace.openSavedQuery(props.sessionId, rowProps.query) },
          { label: "Edit", icon: Pencil, onSelect: () => setEditing({ query: rowProps.query }) },
          { label: "Rename", icon: TextCursorInput, onSelect: () => rename(rowProps.query) },
          { separator: true },
          { label: "Delete", icon: Trash2, danger: true, onSelect: () => remove(rowProps.query) },
        ])}
      >
        <span class={styles.QueryName}>{rowProps.query.name}</span>
        <span class={styles.QueryMeta}>
          {rowProps.query.database ?? "No database"} · {rowProps.query.sql.replace(/\s+/g, " ").slice(0, 80)}
        </span>
      </div>
    );
  }

  return (
    <>
      <div class={styles.Filter}>
        <Input
          type="search"
          value={filter()}
          onValue={setFilter}
          placeholder="Filter saved queries"
          before={<Search size={14} />}
          style={{ flex: "1" }}
        />
        <Button icon tooltip="New saved query" onClick={() => setEditing({ query: null })}>
          <Plus size={16} />
        </Button>
      </div>
      <div class={styles.Tree}>
        <Show
          when={matching().length > 0}
          fallback={
            <EmptyState
              title={saved.queries.length === 0 ? "No saved queries" : "Nothing matches"}
              description={saved.queries.length === 0 ? "Press Ctrl+S in a query tab, or use + to write one here." : undefined}
            />
          }
        >
          <Show when={ownQueries().length > 0}>
            <div class={styles.GroupLabel}>This connection</div>
            <For each={ownQueries()}>{query => <QueryRow query={query} />}</For>
          </Show>
          <Show when={otherQueries().length > 0}>
            <div class={styles.GroupLabel}>Other connections</div>
            <For each={otherQueries()}>{query => <QueryRow query={query} />}</For>
          </Show>
        </Show>
      </div>

      <SavedQueryEditor sessionId={props.sessionId} state={editing()} onClose={() => setEditing(null)} />
    </>
  );
}

export default SavedQueriesPanel;
