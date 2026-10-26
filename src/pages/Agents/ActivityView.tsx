import styles from "./Activity.module.scss";
import { For, Show, createEffect, createMemo, createResource, createSignal, on, onCleanup } from "solid-js";
import { ChevronLeft, ChevronRight, CircleAlert, History, Plug, Search, TriangleAlert, X } from "lucide-solid";
import { AgentApproval, AgentHistoryEntry, AgentMode } from "@core/types";
import { safeInvoke } from "@core/helpers/safeInvoke";
import { errorMessage } from "@core/helpers/errors";
import { formatAgo, plural } from "@core/helpers";
import useAgentState from "@core/states/agentState";
import useConnectionsState from "@core/states/connectionsState";
import useToastState from "@core/states/toastState";
import Button from "@core/components/Button/Button";
import Input from "@core/components/Input/Input";
import SegmentedControl from "@core/components/SegmentedControl/SegmentedControl";
import EmptyState from "@core/components/EmptyState/EmptyState";
import Spinner from "@core/components/Spinner/Spinner";
import AgentMark from "./AgentMark";
import CallRow from "./CallRow";
import {
  DAY_MS, MODE_LABELS, approvalSummary, formatClock, isChange, lowerFirst, startOfDay, tableOf, where,
} from "./agentText";

type CallFilter = "all" | "changes" | "failed";

const ALL = "all";
const TICK_MS = 15_000;

type PastSession = {
  id: string,
  agent: string,
  connectionName: string | null,
  database: string | null,
  openedAt: number,
  closedAt: number | null,
  closedHow: string | null,
};

function pastSessions(entries: AgentHistoryEntry[], openIds: Set<string>): PastSession[] {
  const sessions = new Map<string, PastSession>();
  // Oldest first, so the first database seen is the one the session started in.
  for (const entry of [...entries].reverse()) {
    const id = entry.sessionId;
    if (!id || openIds.has(id)) continue;
    const session = sessions.get(id) ?? {
      id,
      agent: entry.agent ?? "Unknown agent",
      connectionName: entry.connectionName,
      database: null,
      openedAt: entry.at,
      closedAt: null,
      closedHow: null,
    };
    session.database ??= entry.database;
    if (entry.event === "sessionClosed") {
      session.closedAt = entry.at;
      session.closedHow = entry.message;
    } else if (entry.tool === "close_session" && entry.outcome === "ok") {
      session.closedAt = entry.at;
      session.closedHow = "Closed by the agent";
    }
    sessions.set(id, session);
  }
  return [...sessions.values()].sort((a, b) => (b.closedAt ?? b.openedAt) - (a.closedAt ?? a.openedAt));
}

function matches(entry: AgentHistoryEntry, needle: string): boolean {
  if (!needle) return true;
  const haystack = [entry.tool, entry.message, entry.connectionName, ...entry.statements, JSON.stringify(entry.arguments ?? "")];
  return haystack.some(text => text?.toLowerCase().includes(needle));
}

function safetyText(approval: AgentApproval): string {
  if (approval.safety.kind === "transaction") return "runs in one transaction";
  return approval.safety.atomic ? "each statement is all or nothing" : "a failed statement can be left half done";
}

function ApprovalBanner(props: { approval: AgentApproval }) {
  const agent = useAgentState;
  const { pushError } = useToastState;
  const target = createMemo(() => props.approval.preview?.table ?? tableOf(props.approval.arguments));

  async function reject() {
    try {
      await agent.answer(props.approval.id, false);
    } catch (error) {
      pushError(error);
    }
  }

  return (
    <div class={styles.Approval}>
      <TriangleAlert size={16} />
      <div class={styles.ApprovalText}>
        <div class={styles.ApprovalTitle}>
          {props.approval.agent} wants to change {where(props.approval.connectionName, props.approval.database)}
        </div>
        <div class={styles.ApprovalSub}>
          {props.approval.tool}{target() ? ` on ${target()}` : ""}, {plural(props.approval.statements.length, "statement")}, {safetyText(props.approval)}
        </div>
      </div>
      <Button size="sm" variant="subtle" onClick={reject}>Reject</Button>
      <Button size="sm" variant="primary" onClick={() => agent.review(props.approval.id)}>Review</Button>
    </div>
  );
}

function WaitingRow(props: { approval: AgentApproval, showAgent: boolean }) {
  const agent = useAgentState;
  const summary = createMemo(() => approvalSummary(props.approval));

  return (
    <div class={styles.Call}>
      <button class={styles.CallRow} onClick={() => agent.review(props.approval.id)}>
        <ChevronRight size={14} />
        <span class={styles.Time}>{formatClock(props.approval.requestedAt)}</span>
        <span class={styles.Tool}>{props.approval.tool}</span>
        <span class={styles.What}>
          <Show when={summary().code}>
            <code>{summary().code}</code>
          </Show>
          <Show when={summary().code && summary().text}>, </Show>
          {summary().text}
        </span>
        <Show when={props.showAgent}>
          <span class={styles.Agent}>{props.approval.agent}</span>
        </Show>
        <span class={styles.Duration} />
        <span class={styles.Outcome} data-tone="wait">Waiting</span>
      </button>
    </div>
  );
}

function ActivityView() {
  const agent = useAgentState;
  const connections = useConnectionsState;
  const [day, setDay] = createSignal(startOfDay(Date.now()));
  const [selected, setSelected] = createSignal<string>(ALL);
  const [filter, setFilter] = createSignal<CallFilter>("all");
  const [search, setSearch] = createSignal("");
  const [openId, setOpenId] = createSignal<string | null>(null);
  const [now, setNow] = createSignal(Date.now());

  const timer = setInterval(() => setNow(Date.now()), TICK_MS);
  onCleanup(() => clearInterval(timer));

  const isToday = createMemo(() => day() === startOfDay(now()));
  const dayLabel = createMemo(() => {
    const date = new Date(day()).toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" });
    return isToday() ? `Today, ${date}` : date;
  });

  const [entries, { mutate, refetch }] = createResource(
    () => (agent.config.historyEnabled ? day() : null),
    from => safeInvoke("list_agent_history", { from, to: from + DAY_MS - 1 }),
  );

  onCleanup(agent.onHistoryAdded(entry => {
    if (entries.state !== "ready" || entry.at < day() || entry.at >= day() + DAY_MS) return;
    mutate(list => [entry, ...(list ?? [])]);
  }));

  createEffect(on(() => agent.config.historyEnabled, enabled => {
    if (enabled) refetch();
  }, { defer: true }));

  const openIds = createMemo(() => new Set(agent.sessions.map(session => session.id)));
  const past = createMemo(() => pastSessions(entries() ?? [], openIds()));
  const callCount = createMemo(() => (entries() ?? []).filter(entry => entry.event === "call").length);

  const openSession = createMemo(() => agent.sessions.find(session => session.id === selected()));
  const pastSession = createMemo(() => past().find(session => session.id === selected()));
  const sessionId = createMemo(() => openSession()?.id ?? pastSession()?.id ?? null);

  createEffect(() => {
    const settled = entries.state === "ready" || !agent.config.historyEnabled;
    if (selected() !== ALL && sessionId() === null && settled) setSelected(ALL);
  });

  const approvals = createMemo(() => agent.approvals.filter(approval => sessionId() === null || approval.sessionId === sessionId()));

  const calls = createMemo(() => {
    const needle = search().trim().toLowerCase();
    return (entries() ?? []).filter(entry => entry.event === "call"
      && (sessionId() === null || entry.sessionId === sessionId())
      && (filter() !== "changes" || isChange(entry))
      && (filter() !== "failed" || entry.outcome !== "ok")
      && matches(entry, needle));
  });
  const waiting = createMemo(() => (isToday() && filter() !== "failed" ? approvals() : []));

  const mode = createMemo<AgentMode | null>(() => {
    const session = openSession();
    if (!session) return null;
    const access = connections.find(session.connectionId)?.agentAccess.mode ?? "off";
    return access === "ask" && !agent.config.allowChanges ? "readOnly" : access;
  });

  const title = createMemo(() => {
    const open = openSession();
    if (open) return `${open.agent} on ${where(open.connectionName, open.catalog ?? open.defaultDatabase)}`;
    const closed = pastSession();
    if (closed) return `${closed.agent} on ${where(closed.connectionName, closed.database)}`;
    return "All activity";
  });

  const meta = createMemo(() => {
    const open = openSession();
    if (open) {
      return `Opened ${formatClock(open.openedAt, false)}, last used ${formatAgo(open.lastUsed, now())}, ${MODE_LABELS[mode() ?? "off"]}`;
    }
    const closed = pastSession();
    if (closed?.closedHow && closed.closedAt) {
      return `Opened ${formatClock(closed.openedAt, false)}, ${lowerFirst(closed.closedHow)} at ${formatClock(closed.closedAt, false)}`;
    }
    if (closed) return `Opened ${formatClock(closed.openedAt, false)}`;
    return "Every agent and session";
  });

  const markAgent = createMemo(() => openSession()?.agent ?? pastSession()?.agent ?? null);
  const filtering = createMemo(() => filter() !== "all" || search().trim() !== "");
  const firstLoad = createMemo(() => entries.loading && entries.latest === undefined);
  const empty = createMemo(() => entries.state === "ready" && calls().length === 0 && waiting().length === 0);

  const emptyIcon = createMemo(() => (filtering() ? Search : History));
  const serverOff = createMemo(() => !filtering() && !agent.status().running && isToday());

  const emptyTitle = createMemo(() => {
    if (filtering()) return "No calls match";
    if (!agent.status().running && isToday()) return "The MCP server is off";
    return isToday() ? "No calls yet today" : "Nothing on this day";
  });

  const emptyDescription = createMemo(() => {
    if (filtering()) return "Try another filter or clear the search.";
    if (!agent.status().running && isToday()) return "Turn it on to let coding agents use the connections you share with them.";
    return "Agent calls show up here as they happen.";
  });

  function moveDay(step: number) {
    setOpenId(null);
    setDay(current => startOfDay(current + step * DAY_MS + DAY_MS / 2));
  }

  function select(id: string) {
    setSelected(id);
    setOpenId(null);
  }

  return (
    <div class={styles.Activity}>
      <aside class={styles.Rail}>
        <div class={styles.RailBody}>
          <button class={styles.SessionRow} classList={{ [styles.Selected]: selected() === ALL }} onClick={() => select(ALL)}>
            <AgentMark agent={null} />
            <div class={styles.SessionText}>
              <div class={styles.SessionName}>All activity</div>
              <div class={styles.SessionSub}>
                <Show when={agent.config.historyEnabled} fallback="History is off">
                  Every agent, {plural(callCount(), "call")} {isToday() ? "today" : "this day"}
                </Show>
              </div>
            </div>
          </button>

          <div class={styles.RailLabel}>
            Open sessions
            <span class={styles.Pill}>{agent.sessions.length}</span>
          </div>
          <Show when={agent.sessions.length > 0} fallback={<div class={styles.RailEmpty}>No agent has a session open.</div>}>
            <For each={agent.sessions}>
              {session => (
                <div
                  class={styles.SessionRow}
                  classList={{ [styles.Selected]: selected() === session.id }}
                  role="button"
                  tabIndex={0}
                  onClick={() => select(session.id)}
                  onKeyDown={event => {
                    if (event.target === event.currentTarget && (event.key === "Enter" || event.key === " ")) select(session.id);
                  }}
                >
                  <AgentMark agent={session.agent} />
                  <div class={styles.SessionText}>
                    <div class={styles.SessionName}>
                      <span class={styles.SessionAgent}>{session.agent}</span>
                      <span class={styles.LiveDot} />
                    </div>
                    <div class={styles.SessionSub}>
                      <b>{where(session.connectionName, session.catalog ?? session.defaultDatabase)}</b>, used {formatAgo(session.lastUsed, now())}
                    </div>
                  </div>
                  <Button
                    icon
                    size="sm"
                    class={styles.RowClose}
                    tooltip="Close session"
                    onClick={event => {
                      event.stopPropagation();
                      agent.closeSession(session.id);
                    }}
                  >
                    <X size={14} />
                  </Button>
                </div>
              )}
            </For>
          </Show>

          <Show when={past().length > 0}>
            <div class={styles.RailLabel}>
              {isToday() ? "Closed today" : "Closed"}
              <span class={styles.Pill}>{past().length}</span>
            </div>
            <For each={past()}>
              {session => (
                <button
                  class={styles.SessionRow}
                  classList={{ [styles.Selected]: selected() === session.id, [styles.Closed]: true }}
                  onClick={() => select(session.id)}
                >
                  <AgentMark agent={session.agent} dim />
                  <div class={styles.SessionText}>
                    <div class={styles.SessionName}><span class={styles.SessionAgent}>{session.agent}</span></div>
                    <div class={styles.SessionSub}>
                      <b>{where(session.connectionName, session.database)}</b>
                      <Show when={session.closedHow && session.closedAt} fallback={`, opened at ${formatClock(session.openedAt, false)}`}>
                        , {lowerFirst(session.closedHow ?? "")} at {formatClock(session.closedAt ?? 0, false)}
                      </Show>
                    </div>
                  </div>
                </button>
              )}
            </For>
          </Show>
        </div>
      </aside>

      <section class={styles.Feed}>
        <For each={approvals()}>
          {approval => <ApprovalBanner approval={approval} />}
        </For>

        <div class={styles.FeedHeader}>
          <AgentMark agent={markAgent()} />
          <div class={styles.FeedTitles}>
            <h2>{title()}</h2>
            <div class={styles.Meta}>{meta()}</div>
          </div>
          <Show when={openSession()}>
            {session => (
              <Button size="sm" variant="subtle" onClick={() => agent.closeSession(session().id)}>
                <X size={13} />
                Close session
              </Button>
            )}
          </Show>
        </div>

        <Show
          when={agent.config.historyEnabled}
          fallback={
            <div class={styles.FeedEmpty}>
              <EmptyState
                icon={History}
                title="History is off"
                description="Turn on Keep history to see every call agents make, the SQL it ran and what they got back."
                actions={<Button variant="subtle" onClick={() => agent.setView("server")}>Open Server</Button>}
              />
            </div>
          }
        >
          <div class={styles.Toolbar}>
            <div class={styles.DayNav}>
              <Button icon size="sm" tooltip="Previous day" onClick={() => moveDay(-1)}>
                <ChevronLeft size={16} />
              </Button>
              <span class={styles.Day}>{dayLabel()}</span>
              <Button icon size="sm" tooltip="Next day" disabled={isToday()} onClick={() => moveDay(1)}>
                <ChevronRight size={16} />
              </Button>
              <Show when={!isToday()}>
                <Button size="sm" onClick={() => setDay(startOfDay(Date.now()))}>Today</Button>
              </Show>
            </div>
            <div class={styles.Spacer} />
            <SegmentedControl<CallFilter>
              value={filter()}
              onChange={setFilter}
              options={[
                { value: "all", label: "All" },
                { value: "changes", label: "Changes" },
                { value: "failed", label: "Failed" },
              ]}
            />
            <Input
              class={styles.Search}
              value={search()}
              onValue={setSearch}
              placeholder="Filter by tool or SQL"
              clearable
              before={<Search size={14} class={styles.SearchIcon} />}
            />
          </div>

          <div class={styles.FeedList}>
            <Show when={!entries.error} fallback={<EmptyState icon={CircleAlert} tone="danger" title="History couldn't be read" description={errorMessage(entries.error)} />}>
              <Show when={firstLoad()}>
                <div class={styles.Loading}><Spinner size={16} /></div>
              </Show>
              <Show when={empty()}>
                <div class={styles.FeedEmpty}>
                  <EmptyState
                    icon={emptyIcon()}
                    title={emptyTitle()}
                    description={emptyDescription()}
                    actions={
                      <Show when={serverOff()}>
                        <Button variant="primary" onClick={() => agent.update({ enabled: true })}>
                          <Plug size={14} />
                          Turn on
                        </Button>
                      </Show>
                    }
                  />
                </div>
              </Show>
              <Show when={calls().length > 0 || waiting().length > 0}>
                <div class={styles.Calls}>
                  <For each={waiting()}>
                    {approval => <WaitingRow approval={approval} showAgent={sessionId() === null} />}
                  </For>
                  <For each={calls()}>
                    {entry => (
                      <CallRow
                        entry={entry}
                        open={openId() === entry.id}
                        showAgent={sessionId() === null}
                        onToggle={() => setOpenId(openId() === entry.id ? null : entry.id)}
                      />
                    )}
                  </For>
                </div>
              </Show>
            </Show>
          </div>
        </Show>
      </section>
    </div>
  );
}

export default ActivityView;
