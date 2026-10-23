import styles from "./TableDataTab.module.scss";
import { Match, Show, Switch, createMemo, createSignal, onCleanup, onMount, untrack } from "solid-js";
import { ArrowLeft, ArrowRight, CircleAlert, Lock, Plus, RefreshCw, Search, SearchX, SquarePen, X } from "lucide-solid";
import { databaseModule } from "@core/databases";
import { safeInvoke } from "@core/helpers/safeInvoke";
import { errorMessage } from "@core/helpers/errors";
import { createRowEdits, previewRowChanges, runRowChanges } from "@core/helpers/rowEdits";
import { formatDuration, plural } from "@core/helpers";
import useWorkspaceState from "@core/states/workspaceState";
import usePromptState from "@core/states/promptState";
import useToastState from "@core/states/toastState";
import { ResultSet } from "@core/types";
import ResultGrid from "@core/components/ResultGrid/ResultGrid";
import ReviewSqlModal from "@core/components/ReviewSqlModal/ReviewSqlModal";
import PendingChanges from "@core/components/PendingChanges/PendingChanges";
import Button from "@core/components/Button/Button";
import Input from "@core/components/Input/Input";
import Select from "@core/components/Select/Select";
import Spinner from "@core/components/Spinner/Spinner";
import EmptyState from "@core/components/EmptyState/EmptyState";

const PAGE_SIZES = [
  { value: 100, label: "100 rows" },
  { value: 500, label: "500 rows" },
  { value: 1000, label: "1,000 rows" },
  { value: 5000, label: "5,000 rows" },
];

const SEARCH_DELAY = 300;

export type TableDataTabProps = {
  sessionId: string,
  database: string,
  table: string,
};

function TableDataTab(props: TableDataTabProps) {
  const workspace = useWorkspaceState;
  const { confirm } = usePromptState;
  const { pushToast, pushError } = useToastState;
  const kind = untrack(() => workspace.store.sessions[props.sessionId].info.kind);
  const module = databaseModule(kind);

  const [pageSize, setPageSize] = createSignal(500);
  const [offset, setOffset] = createSignal(0);
  const [result, setResult] = createSignal<ResultSet | null>(null);
  const [loading, setLoading] = createSignal(false);
  const [error, setError] = createSignal<unknown>(null);
  const [elapsed, setElapsed] = createSignal(0);
  const [review, setReview] = createSignal<string[] | null>(null);
  const [searchText, setSearchText] = createSignal("");
  const [search, setSearch] = createSignal("");

  const edits = createRowEdits(result);
  const editable = createMemo(() => result()?.columns.some(column => column.primaryKey) ?? false);
  const gridEditing = createMemo(() => (editable() ? edits.editing : undefined));
  const noMatches = createMemo(() => search() !== "" && result()?.rows.length === 0 && edits.changeCount() === 0);

  let request = 0;
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  onCleanup(() => clearTimeout(searchTimer));

  async function load(nextOffset = offset(), size = pageSize(), term = search()) {
    const id = ++request;
    setLoading(true);
    setError(null);
    const started = performance.now();

    try {
      const rows = await safeInvoke("fetch_table_rows", {
        sessionId: props.sessionId,
        database: props.database,
        table: props.table,
        offset: nextOffset,
        limit: size,
        search: term || null,
      });
      if (id !== request) return;
      setResult(rows);
      setOffset(nextOffset);
      setSearch(term);
      setElapsed(performance.now() - started);
    } catch (loadError) {
      if (id === request) setError(loadError);
    } finally {
      if (id === request) setLoading(false);
    }
  }

  onMount(() => load(0));

  async function leaveEdits(): Promise<boolean> {
    if (edits.changeCount() === 0) return true;
    return confirm({
      title: "Discard changes?",
      message: `${plural(edits.changeCount(), "pending change")} will be lost.`,
      confirmLabel: "Discard",
      danger: true,
    });
  }

  async function reload(nextOffset = offset(), size = pageSize(), term = search()) {
    if (await leaveEdits()) await load(nextOffset, size, term);
  }

  function onSearchInput(text: string) {
    setSearchText(text);
    clearTimeout(searchTimer);
    if (edits.changeCount() > 0) return;
    searchTimer = setTimeout(() => {
      const term = text.trim();
      if (term !== search()) load(0, pageSize(), term);
    }, SEARCH_DELAY);
  }

  async function submitSearch(text: string) {
    clearTimeout(searchTimer);
    setSearchText(text);
    const term = text.trim();
    if (term !== search()) await reload(0, pageSize(), term);
  }

  async function startReview() {
    const current = result();
    if (!current) return;
    try {
      setReview(await previewRowChanges(
        props.sessionId,
        { database: props.database, table: props.table, columns: current.columns },
        edits.changes(),
      ));
    } catch (reviewError) {
      pushError(reviewError);
    }
  }

  async function apply(): Promise<string | null> {
    const statements = review();
    if (!statements) return null;

    const failure = await runRowChanges(props.sessionId, props.database, statements);
    if (failure) return failure;

    pushToast(`Applied ${plural(edits.changeCount(), "change")}`, "success", 2500);
    await load();
    return null;
  }

  function openAsQuery() {
    workspace.newQueryTab(props.sessionId, props.database, module.selectRowsSql(props.table));
  }

  return (
    <div class={styles.TableDataTab}>
      <div class={styles.Toolbar}>
        <Button size="sm" onClick={() => reload()} disabled={loading()} tooltip="Reload">
          <RefreshCw size={14} />
        </Button>
        <Select
          size="sm"
          value={pageSize()}
          items={PAGE_SIZES}
          onChange={async size => {
            if (!(await leaveEdits())) return;
            setPageSize(size);
            load(0, size);
          }}
        />
        <div class={styles.Pager}>
          <Button
            icon
            size="sm"
            tooltip="Previous page"
            disabled={loading() || offset() === 0}
            onClick={() => reload(Math.max(0, offset() - pageSize()))}
          >
            <ArrowLeft size={14} />
          </Button>
          <span class={styles.Range}>
            <Show when={result()} fallback="">
              {current => current().rows.length === 0
                ? "No rows"
                : `${(offset() + 1).toLocaleString()} to ${(offset() + current().rows.length).toLocaleString()}`}
            </Show>
          </span>
          <Button
            icon
            size="sm"
            tooltip="Next page"
            disabled={loading() || (result()?.rows.length ?? 0) < pageSize()}
            onClick={() => reload(offset() + pageSize())}
          >
            <ArrowRight size={14} />
          </Button>
        </div>
        <div class={styles.Separator} />
        <Input
          class={`${styles.Search} ${search() !== "" ? styles.Searching : ""}`}
          value={searchText()}
          placeholder={`Search ${props.table}`}
          onValue={onSearchInput}
          onKeyDown={event => {
            if (event.key === "Enter") submitSearch(event.currentTarget.value);
            else if (event.key === "Escape" && searchText() !== "") {
              event.stopPropagation();
              submitSearch("");
            }
          }}
          before={<Search size={14} />}
          after={
            <Show when={searchText() !== ""}>
              <button class={styles.ClearSearch} title="Clear search" onClick={() => submitSearch("")}>
                <X size={12} />
              </button>
            </Show>
          }
        />
        <Show
          when={editable()}
          fallback={
            <Show when={result()}>
              <span class={styles.ReadOnly} title="Rows can only be edited when the table has a primary key">
                <Lock size={12} />
                Read only
              </span>
            </Show>
          }
        >
          <Button size="sm" onClick={edits.addRow} tooltip="Add a row">
            <Plus size={14} />
            Add row
          </Button>
        </Show>
        <div class={styles.Spacer} />
        <Show when={edits.changeCount() > 0}>
          <PendingChanges count={edits.changeCount()} onDiscard={edits.discard} onApply={startReview} />
          <div class={styles.Separator} />
        </Show>
        <Show when={loading()} fallback={<span class={styles.Timing}>{result() ? formatDuration(elapsed()) : ""}</span>}>
          <Spinner size={12} />
        </Show>
        <Button size="sm" onClick={openAsQuery} tooltip="Open this table in a query editor">
          <SquarePen size={14} />
          Query
        </Button>
      </div>

      <div class={styles.Content}>
        <Switch>
          <Match when={error()}>
            <EmptyState
              tone="danger"
              icon={CircleAlert}
              title={`Couldn't load ${props.table}`}
              description={errorMessage(error())}
              actions={<Button variant="subtle" onClick={() => load()}>Try again</Button>}
            />
          </Match>
          <Match when={noMatches()}>
            <EmptyState
              icon={SearchX}
              title="No matching rows"
              description={offset() > 0
                ? `No more rows in ${props.table} contain "${search()}".`
                : `No row in ${props.table} contains "${search()}".`}
              actions={<Button variant="subtle" onClick={() => submitSearch("")}>Clear search</Button>}
            />
          </Match>
          <Match when={result()}>
            {current => (
              <ResultGrid
                columns={current().columns}
                rows={edits.rows()}
                rowOffset={offset()}
                editing={gridEditing()}
                highlight={search()}
                sessionId={props.sessionId}
              />
            )}
          </Match>
          <Match when={loading()}>
            <div class={styles.Center}><Spinner size={18} /></div>
          </Match>
        </Switch>
      </div>

      <ReviewSqlModal
        statements={review()}
        title="Apply changes"
        description={`Row changes to ${props.database}.${props.table}`}
        dialect={module.sqlDialect}
        safety={{ kind: "transaction" }}
        onApply={apply}
        onClose={() => setReview(null)}
      />
    </div>
  );
}

export default TableDataTab;
