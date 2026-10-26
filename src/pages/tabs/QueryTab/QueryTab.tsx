import styles from "./QueryTab.module.scss";
import { Accessor, For, Match, Show, Switch, createEffect, createMemo, createSignal, onCleanup, untrack } from "solid-js";
import {
  ArrowDownToLine, CircleAlert, CircleCheck, Lock, Pencil, Play, Plus, Save, Search, Square, StepForward, Table2, X,
} from "lucide-solid";
import { databaseModule } from "@core/databases";
import { safeInvoke } from "@core/helpers/safeInvoke";
import { errorMessage, isAppError } from "@core/helpers/errors";
import { formatDuration, plural } from "@core/helpers";
import { buildCatalog, buildCompletionSchema } from "@core/helpers/catalog";
import {
  EditCheck, EditTarget, RowEdits, createRowEdits, findEditTarget, previewRowChanges, runRowChanges,
} from "@core/helpers/rowEdits";
import useWorkspaceState, { QueryTabState } from "@core/states/workspaceState";
import useSavedQueriesState from "@core/states/savedQueriesState";
import usePromptState from "@core/states/promptState";
import useToastState from "@core/states/toastState";
import { ResultSet, StatementOutcome, StatementRange } from "@core/types";
import { statementAt } from "@core/helpers/statements";
import SqlEditor, { SqlEditorApi } from "@core/components/SqlEditor/SqlEditor";
import ResultGrid, { GridEditing } from "@core/components/ResultGrid/ResultGrid";
import ReviewSqlModal from "@core/components/ReviewSqlModal/ReviewSqlModal";
import PendingChanges from "@core/components/PendingChanges/PendingChanges";
import Button from "@core/components/Button/Button";
import Input from "@core/components/Input/Input";
import Select from "@core/components/Select/Select";
import Spinner from "@core/components/Spinner/Spinner";
import EmptyState from "@core/components/EmptyState/EmptyState";
import { tooltip } from "@core/components/Tooltip/Tooltip";

const ROW_LIMITS = [
  { value: 100, label: "Limit 100" },
  { value: 500, label: "Limit 500" },
  { value: 1000, label: "Limit 1,000" },
  { value: 5000, label: "Limit 5,000" },
  { value: 10000, label: "Limit 10,000" },
  { value: 0, label: "No limit" },
];

export type QueryTabProps = {
  sessionId: string,
  database: string,
  tab: QueryTabState,
  active: boolean,
};

type ResultEntry = {
  set: ResultSet,
  label: string,
  edits: RowEdits,
  editing: Accessor<GridEditing | undefined>,
  target: Accessor<EditTarget | null>,
  readOnlyReason: Accessor<string | null>,
  replaceRows: (rows: ResultSet["rows"]) => void,
};

type Review = { entry: ResultEntry, target: EditTarget, statements: string[] };
type RunMode = "current" | "after" | "all";
type RunStat = { label: string, value: string, hint: string };

const SPLIT_DELAY = 200;
type View = number | "messages";

function outcomeSummary(outcome: StatementOutcome): string {
  if (outcome.error) {
    const detail = outcome.error.detail;
    const errorCode = detail.kind === "database" ? detail.code ?? detail.sqlState : null;
    const code = errorCode !== null ? `Error ${errorCode}: ` : "";
    return `${code}${outcome.error.message}`;
  }

  if (outcome.resultSets.length > 0) {
    return outcome.resultSets
      .map(set => `${plural(set.rows.length, "row")} returned${set.truncated ? " (limit reached)" : ""}`)
      .join(", ");
  }

  const parts = [`${plural(outcome.affectedRows, "row")} affected`];
  if (outcome.lastInsertId) parts.push(`last insert id ${outcome.lastInsertId}`);
  if (outcome.warnings > 0) parts.push(plural(outcome.warnings, "warning"));
  if (outcome.info) parts.push(outcome.info);
  return parts.join(", ");
}

function QueryTab(props: QueryTabProps) {
  const workspace = useWorkspaceState;
  const savedQueries = useSavedQueriesState;
  const { askForText, confirm } = usePromptState;
  const { pushError, pushToast } = useToastState;

  const [running, setRunning] = createSignal(false);
  const [startedAt, setStartedAt] = createSignal(0);
  const [now, setNow] = createSignal(Date.now());
  const [rowLimit, setRowLimit] = createSignal(1000);
  const [outcomes, setOutcomes] = createSignal<StatementOutcome[]>([]);
  const [runError, setRunError] = createSignal<unknown>(null);
  const [view, setView] = createSignal<View>("messages");
  const [editorShare, setEditorShare] = createSignal(0.45);
  const [hasRun, setHasRun] = createSignal(false);
  const [saving, setSaving] = createSignal(false);
  const [review, setReview] = createSignal<Review | null>(null);
  const [roundTrip, setRoundTrip] = createSignal(0);
  const [filterText, setFilterText] = createSignal("");
  const filter = createMemo(() => filterText().trim());

  const [statementRanges, setStatementRanges] = createSignal<StatementRange[]>([]);

  let editor: SqlEditorApi | undefined;
  let contextId: number | null = null;
  let splitTimer: ReturnType<typeof setTimeout> | undefined;
  onCleanup(() => clearTimeout(splitTimer));
  let body!: HTMLDivElement;

  const session = createMemo(() => workspace.store.sessions[props.sessionId]);
  const kind = untrack(() => workspace.store.sessions[props.sessionId].info.kind);
  const dialect = databaseModule(kind).sqlDialect;

  const catalog = createMemo(() => {
    const current = session();
    return current ? buildCatalog(current, props.database) : null;
  });
  const completionSchema = createMemo(() => {
    const current = session();
    return current ? buildCompletionSchema(current) : undefined;
  });

  function createResultEntry(set: ResultSet, label: string): ResultEntry {
    const [rows, setRows] = createSignal(set.rows);
    const source = createMemo<ResultSet>(() => ({ ...set, rows: rows() }));
    const [check, setCheck] = createSignal<EditCheck | null>(null);
    const target = createMemo(() => {
      const current = check();
      return current && "target" in current ? current.target : null;
    });
    const readOnlyReason = createMemo(() => {
      const current = check();
      return current && "reason" in current ? current.reason : null;
    });

    if (set.columns.length > 0) {
      findEditTarget(props.sessionId, set.columns).then(setCheck, error => setCheck({ reason: errorMessage(error) }));
    }

    const edits = createRowEdits(source);
    const editing = createMemo(() => (target() ? edits.editing : undefined));

    return { set, label, edits, editing, target, readOnlyReason, replaceRows: setRows };
  }

  const resultEntries = createMemo<ResultEntry[]>(() => {
    const entries: ResultEntry[] = [];
    for (const outcome of outcomes()) {
      for (const set of outcome.resultSets) entries.push(createResultEntry(set, `Result ${entries.length + 1}`));
    }
    return entries;
  });

  const runStats = createMemo<RunStat[]>(() => {
    const ran = outcomes();
    if (ran.length === 0) return [];
    const server = ran.reduce((sum, outcome) => sum + outcome.executionMs, 0);
    const fetch = ran.reduce((sum, outcome) => sum + outcome.fetchMs, 0);
    const network = ran[0].networkMs;

    const stats: RunStat[] = [
      { label: "Server", value: formatDuration(server), hint: "Time until the server replied, with the network round trip taken out" },
    ];
    if (network !== null) {
      stats.push({ label: "Network", value: formatDuration(network), hint: "One round trip to the server, measured with a ping before the run" });
    }
    stats.push(
      { label: "Fetch", value: formatDuration(fetch), hint: "Receiving and converting the rows after the server replied" },
      { label: "Total", value: formatDuration(roundTrip()), hint: "From sending the query until the results reached the app" },
    );
    return stats;
  });
  const failed = createMemo(() => runError() !== null || outcomes().some(outcome => outcome.error));
  const pendingChanges = createMemo(() => resultEntries().reduce((sum, entry) => sum + entry.edits.changeCount(), 0));
  const activeResult = createMemo(() => {
    const current = view();
    return typeof current === "number" ? resultEntries()[current] : undefined;
  });

  createEffect(() => {
    if (!running()) return;
    const timer = setInterval(() => setNow(Date.now()), 100);
    onCleanup(() => clearInterval(timer));
  });

  createEffect(() => {
    if (props.active) queueMicrotask(() => editor?.focus());
  });

  onCleanup(() => {
    if (contextId !== null) {
      safeInvoke("close_query_context", { sessionId: props.sessionId, contextId }).catch(() => {});
    }
  });

  // Saving copies the text into a new saved query. The tab stays an ordinary tab.
  async function saveAsQuery() {
    if (!editor || saving()) return;
    const sql = editor.text();
    const profile = session()?.profile;
    if (!profile || !sql.trim()) return;

    const name = await askForText({ title: "Save as query", label: "Name", initial: props.tab.title, confirmLabel: "Save" });
    if (!name) return;

    setSaving(true);
    try {
      const saved = await savedQueries.save({
        id: "",
        name,
        sql,
        connectionId: profile.id,
        database: props.database,
        updatedAt: 0,
      });
      pushToast(`Saved as ${saved.name}`, "success", 2000);
    } catch (error) {
      pushError(error);
    } finally {
      setSaving(false);
    }
  }

  async function ensureContext(): Promise<number> {
    if (contextId === null) {
      contextId = await safeInvoke("open_query_context", { sessionId: props.sessionId, database: props.database });
    }
    return contextId;
  }

  function scheduleSplit(text: string) {
    clearTimeout(splitTimer);
    splitTimer = setTimeout(async () => {
      try {
        const ranges = await safeInvoke("split_sql", { kind, sql: text });
        if (editor?.text() === text) setStatementRanges(ranges);
      } catch {
        setStatementRanges([]);
      }
    }, SPLIT_DELAY);
  }

  // "after" runs the statement at the cursor and everything below it, ignoring any selection.
  async function statementToRun(mode: RunMode): Promise<string | null> {
    if (!editor) return null;

    const selection = editor.selection();
    if (mode !== "after" && selection.text.trim()) return selection.text;

    const text = editor.text();
    if (mode === "all") return text;

    const ranges = await safeInvoke("split_sql", { kind, sql: text });
    const index = statementAt(ranges, mode === "after" ? selection.from : editor.cursor());
    if (index === -1) return null;
    return mode === "after" ? text.slice(ranges[index].start) : ranges[index].text;
  }

  async function run(mode: RunMode) {
    if (running()) return;

    if (pendingChanges() > 0) {
      const discard = await confirm({
        title: "Discard changes?",
        message: `${plural(pendingChanges(), "unapplied change")} in the results will be lost.`,
        confirmLabel: "Discard and run",
        danger: true,
      });
      if (!discard) return;
    }

    let sql: string | null;
    try {
      sql = await statementToRun(mode);
    } catch (error) {
      pushError(error);
      return;
    }
    if (!sql?.trim()) return;

    setRunning(true);
    setStartedAt(Date.now());
    setNow(Date.now());
    setRunError(null);
    setHasRun(true);

    try {
      const id = await ensureContext();
      const sent = performance.now();
      const result = await safeInvoke("execute_query", {
        sessionId: props.sessionId,
        contextId: id,
        sql,
        rowLimit: rowLimit(),
      });
      setRoundTrip(performance.now() - sent);
      setOutcomes(result);
      const firstResult = result.some(outcome => outcome.resultSets.length > 0);
      setView(firstResult && !result.some(outcome => outcome.error) ? 0 : "messages");

      const changedSchema = result.some(outcome => !outcome.error && /^\s*(create|alter|drop|rename)\b/i.test(outcome.sql));
      if (changedSchema) workspace.refreshCatalog(props.sessionId, props.database);
    } catch (error) {
      if (isAppError(error) && error.detail.kind === "notFound") contextId = null;
      setOutcomes([]);
      setRunError(error);
      setView("messages");
    } finally {
      setRunning(false);
    }
  }

  async function startReview(entry: ResultEntry) {
    const target = entry.target();
    if (!target) return;
    try {
      setReview({ entry, target, statements: await previewRowChanges(props.sessionId, target, entry.edits.changes()) });
    } catch (error) {
      pushError(error);
    }
  }

  async function applyReview(): Promise<string | null> {
    const current = review();
    if (!current) return null;

    const failure = await runRowChanges(props.sessionId, current.target.database, current.statements);
    if (failure) return failure;

    const applied = current.entry.edits.changeCount();
    const inserted = current.entry.edits.changes().some(change => change.kind === "insert");
    current.entry.replaceRows(current.entry.edits.savedRows());
    pushToast(
      inserted
        ? `Applied ${plural(applied, "change")}. Run the query again to see the new rows.`
        : `Applied ${plural(applied, "change")}`,
      "success",
      inserted ? 4000 : 2500,
    );
    return null;
  }

  async function stop() {
    if (contextId === null) return;
    try {
      await safeInvoke("cancel_query", { sessionId: props.sessionId, contextId });
    } catch (error) {
      pushError(error);
    }
  }

  function startResize(event: MouseEvent) {
    event.preventDefault();
    const rect = body.getBoundingClientRect();

    function onMove(move: MouseEvent) {
      setEditorShare(Math.min(0.85, Math.max(0.12, (move.clientY - rect.top) / rect.height)));
    }

    function onUp() {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      document.body.style.removeProperty("cursor");
    }

    document.body.style.cursor = "row-resize";
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }

  return (
    <div class={styles.QueryTab}>
      <div class={styles.Toolbar}>
        <Button variant="primary" size="sm" onClick={() => run("all")} disabled={running()} tooltip="Run everything, or the selection (Ctrl+Shift+Enter)">
          <Play size={12} />
          Run all
        </Button>
        <Button size="sm" onClick={() => run("current")} disabled={running()} tooltip="Run the highlighted statement, or the selection (Ctrl+Enter)">
          <StepForward size={14} />
          Run
        </Button>
        <Button size="sm" onClick={() => run("after")} disabled={running()} tooltip="Run the highlighted statement and everything after it">
          <ArrowDownToLine size={14} />
          Run all after
        </Button>
        <Button size="sm" onClick={stop} disabled={!running()} tooltip="Stop the running query">
          <Square size={12} />
          Stop
        </Button>
        <div class={styles.Separator} />
        <Select size="sm" value={rowLimit()} items={ROW_LIMITS} onChange={setRowLimit} />
        <Button size="sm" onClick={saveAsQuery} disabled={saving()} tooltip="Save as a new saved query (Ctrl+S)">
          <Save size={14} />
          Save as query
        </Button>
        <div class={styles.Spacer} />
        <Show when={running()}>
          <span class={styles.Status}>
            <Spinner size={12} />
            Running {formatDuration(now() - startedAt())}
          </span>
        </Show>
      </div>

      <div ref={body} class={styles.Body}>
        <div class={styles.EditorPane} style={{ height: `${editorShare() * 100}%` }}>
          <SqlEditor
            initialValue={untrack(() => workspace.queryText(props.tab))}
            dialect={dialect}
            schema={completionSchema()}
            defaultSchema={props.database}
            catalog={catalog()}
            statements={statementRanges()}
            onChange={text => {
              workspace.setQueryText(props.sessionId, props.tab.id, text);
              scheduleSplit(text);
            }}
            onRunCurrent={() => run("current")}
            onRunAll={() => run("all")}
            onSave={saveAsQuery}
            onReady={api => {
              editor = api;
              scheduleSplit(api.text());
              if (props.active) api.focus();
              if (untrack(() => props.tab.runOnOpen)) {
                workspace.clearRunOnOpen(props.sessionId, props.database, props.tab.id);
                run("all");
              }
            }}
          />
        </div>

        <div class={styles.Splitter} onMouseDown={startResize} />

        <div class={styles.ResultsPane}>
          <Show
            when={hasRun()}
            fallback={
              <EmptyState
                title="No results yet"
                description="Ctrl+Enter runs the statement under the cursor. Ctrl+Shift+Enter runs the whole editor or the selection."
              />
            }
          >
            <div class={styles.ResultTabs}>
              <For each={resultEntries()}>
                {(entry, index) => (
                  <button
                    class={styles.ResultTab}
                    classList={{ [styles.Active]: view() === index() }}
                    onClick={() => setView(index())}
                  >
                    <Table2 size={14} />
                    {entry.label}
                    <span class={styles.Count}>{entry.set.rows.length.toLocaleString()}</span>
                  </button>
                )}
              </For>
              <button
                class={styles.ResultTab}
                classList={{ [styles.Active]: view() === "messages", [styles.HasError]: failed() }}
                onClick={() => setView("messages")}
              >
                {failed() ? <CircleAlert size={14} /> : <CircleCheck size={14} />}
                Messages
              </button>
              <div class={styles.Spacer} />
              <Show when={activeResult()}>
                {entry => (
                  <>
                    <Input
                      class={`${styles.Filter} ${filter() ? styles.Filtering : ""}`}
                      value={filterText()}
                      placeholder="Filter rows"
                      onValue={setFilterText}
                      onKeyDown={event => {
                        if (event.key === "Escape" && filterText() !== "") {
                          event.stopPropagation();
                          setFilterText("");
                        }
                      }}
                      before={<Search size={12} />}
                      after={
                        <Show when={filterText() !== ""}>
                          <button
                            class={styles.ClearFilter}
                            ref={element => tooltip(element, () => "Clear filter")}
                            onClick={() => setFilterText("")}
                          >
                            <X size={12} />
                          </button>
                        </Show>
                      }
                    />
                    <Show
                      when={entry().target()}
                      fallback={
                        <Show when={entry().readOnlyReason()}>
                          {reason => (
                            <span class={styles.EditState} ref={element => tooltip(element, reason)}>
                              <Lock size={12} />
                              Read only
                            </span>
                          )}
                        </Show>
                      }
                    >
                      {target => (
                        <>
                          <span
                            class={styles.EditState}
                            ref={element => tooltip(element, () => `Double-click a cell to edit. Changes are written to ${target().database}.${target().table}.`)}
                          >
                            <Pencil size={12} />
                            {target().table}
                          </span>
                          <Button size="sm" class={styles.AddRow} onClick={() => entry().edits.addRow()} tooltip={`Add a row to ${target().table}`}>
                            <Plus size={14} />
                            Add row
                          </Button>
                        </>
                      )}
                    </Show>
                    <Show when={entry().edits.changeCount() > 0}>
                      <PendingChanges
                        count={entry().edits.changeCount()}
                        onDiscard={entry().edits.discard}
                        onApply={() => startReview(entry())}
                      />
                    </Show>
                  </>
                )}
              </Show>
            </div>

            <div class={styles.ResultContent}>
              <Switch>
                <Match when={activeResult()}>
                  {entry => (
                    <>
                      <div class={styles.Grid}>
                        <ResultGrid
                          columns={entry().set.columns}
                          rows={entry().edits.rows()}
                          editing={entry().editing()}
                          highlight={filter()}
                          filter
                          sessionId={props.sessionId}
                        />
                      </div>
                      <Show when={entry().set.truncated}>
                        <div class={styles.Truncated}>
                          Showing the first {entry().set.rows.length.toLocaleString()} rows. Raise the limit to see more.
                        </div>
                      </Show>
                    </>
                  )}
                </Match>
                <Match when={view() === "messages"}>
                  <div class={styles.Messages}>
                    <Show when={runStats().length > 0}>
                      <div class={styles.Stats}>
                        <For each={runStats()}>
                          {stat => (
                            <span class={styles.Stat} ref={element => tooltip(element, () => stat.hint)}>
                              <span class={styles.StatLabel}>{stat.label}</span>
                              <span class={styles.StatValue}>{stat.value}</span>
                            </span>
                          )}
                        </For>
                      </div>
                    </Show>
                    <Show when={runError()}>
                      <div class={`${styles.Message} ${styles.Failed}`}>
                        <CircleAlert size={14} />
                        <span class={styles.MessageText}>{errorMessage(runError())}</span>
                      </div>
                    </Show>
                    <For each={outcomes()}>
                      {(outcome, index) => (
                        <div class={styles.Message} classList={{ [styles.Failed]: outcome.error !== null }}>
                          {outcome.error ? <CircleAlert size={14} /> : <CircleCheck size={14} />}
                          <span class={styles.Index}>{index() + 1}</span>
                          <span class={styles.Sql} ref={element => tooltip(element, () => outcome.sql)}>
                            {outcome.sql.replace(/\s+/g, " ")}
                          </span>
                          <span class={styles.MessageText}>{outcomeSummary(outcome)}</span>
                          <span
                            class={styles.Duration}
                            ref={element => tooltip(element, () => `Server ${formatDuration(outcome.executionMs)}, fetch ${formatDuration(outcome.fetchMs)}`)}
                          >
                            {formatDuration(outcome.executionMs)}
                            <Show when={outcome.fetchMs >= 1}>
                              <span class={styles.FetchPart}> + {formatDuration(outcome.fetchMs)} fetch</span>
                            </Show>
                          </span>
                        </div>
                      )}
                    </For>
                  </div>
                </Match>
              </Switch>
            </div>
          </Show>
        </div>
      </div>

      <ReviewSqlModal
        statements={review()?.statements ?? null}
        title="Apply changes"
        description={review() ? `Row changes to ${review()!.target.database}.${review()!.target.table}` : ""}
        dialect={dialect}
        safety={{ kind: "transaction" }}
        onApply={applyReview}
        onClose={() => setReview(null)}
      />
    </div>
  );
}

export default QueryTab;
