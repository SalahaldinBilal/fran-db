import styles from "./ConnectionCard.module.scss";
import { Show, createMemo } from "solid-js";
import { ArrowRight, Copy, Database, Ellipsis, Pencil, Plug, Server, Trash2 } from "lucide-solid";
import { ConnectionProfile } from "@core/types";
import { withDatabaseModule } from "@core/databases";
import useWorkspaceState from "@core/states/workspaceState";
import useConnectionsState from "@core/states/connectionsState";
import useContextMenuState, { MenuItem } from "@core/states/contextMenuState";
import usePromptState from "@core/states/promptState";
import useToastState from "@core/states/toastState";
import Button from "@core/components/Button/Button";
import Spinner from "@core/components/Spinner/Spinner";

export type ConnectionCardProps = {
  profile: ConnectionProfile,
  onEdit: () => void,
};

function ConnectionCard(props: ConnectionCardProps) {
  const workspace = useWorkspaceState;
  const connections = useConnectionsState;
  const { openContextMenu, openMenuAt } = useContextMenuState;
  const { confirm } = usePromptState;
  const { pushError } = useToastState;

  const details = createMemo(() =>
    withDatabaseModule(props.profile, (module, options) => ({
      kind: module.label,
      target: module.describeTarget(options),
      method: module.methodLabel(options),
    })),
  );
  const connecting = createMemo(() => workspace.store.connecting[props.profile.id] ?? false);

  function connect() {
    if (!connecting()) workspace.openConnection(props.profile);
  }

  async function remove() {
    const confirmed = await confirm({
      title: `Delete ${props.profile.name}?`,
      message: "The connection and its saved passwords will be removed from this machine.",
      confirmLabel: "Delete",
      danger: true,
    });
    if (!confirmed) return;

    try {
      await connections.remove(props.profile.id);
    } catch (error) {
      pushError(error);
    }
  }

  async function duplicate() {
    try {
      await connections.duplicate(props.profile.id);
    } catch (error) {
      pushError(error);
    }
  }

  function menuItems(): MenuItem[] {
    return [
      { label: "Connect", icon: Plug, onSelect: connect },
      { label: "Edit", icon: Pencil, onSelect: props.onEdit },
      { label: "Duplicate", icon: Copy, onSelect: duplicate },
      { separator: true },
      { label: "Delete", icon: Trash2, danger: true, onSelect: remove },
    ];
  }

  return (
    <div
      class={styles.Card}
      classList={{ [styles.Connecting]: connecting() }}
      style={{ "--card-color": props.profile.color ?? "var(--accent)" }}
      role="button"
      tabIndex={0}
      aria-label={`Connect to ${props.profile.name}`}
      onClick={connect}
      onKeyDown={event => {
        if (event.target === event.currentTarget && (event.key === "Enter" || event.key === " ")) {
          event.preventDefault();
          connect();
        }
      }}
      onContextMenu={event => openContextMenu(event, menuItems())}
    >
      <div class={styles.Top}>
        <div class={styles.Icon}>
          <Database size={18} />
        </div>
        <div class={styles.Titles}>
          <div class={styles.Name} title={props.profile.name}>{props.profile.name}</div>
          <div class={styles.Kind}>{details().kind}</div>
        </div>
        <div class={styles.Tools} onClick={event => event.stopPropagation()}>
          <Button icon size="sm" tooltip="Edit" onClick={props.onEdit}>
            <Pencil size={14} />
          </Button>
          <Button
            icon
            size="sm"
            tooltip="More"
            onClick={event => {
              const rect = event.currentTarget.getBoundingClientRect();
              openMenuAt({ x: rect.left, y: rect.bottom + 2 }, menuItems());
            }}
          >
            <Ellipsis size={16} />
          </Button>
        </div>
      </div>

      <div class={styles.Target} title={details().target}>
        <Server size={14} />
        <span>{details().target}</span>
      </div>

      <div class={styles.Bottom}>
        <span class={styles.Method}>{details().method}</span>
        <span class={styles.Status}>
          <Show
            when={connecting()}
            fallback={
              <>
                Connect
                <ArrowRight size={14} class={styles.Arrow} />
              </>
            }
          >
            <Spinner size={12} />
            Connecting
          </Show>
        </span>
      </div>
    </div>
  );
}

export default ConnectionCard;
