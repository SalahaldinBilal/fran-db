import styles from "./Activity.module.scss";
import { For, Show, createMemo, createResource } from "solid-js";
import { ChevronDown, ChevronRight, CircleAlert, ShieldCheck } from "lucide-solid";
import { AgentHistoryEntry } from "@core/types";
import { safeInvoke } from "@core/helpers/safeInvoke";
import { errorMessage } from "@core/helpers/errors";
import { formatDuration, plural } from "@core/helpers";
import CodeView from "@core/components/CodeView/CodeView";
import ResultGrid from "@core/components/ResultGrid/ResultGrid";
import Spinner from "@core/components/Spinner/Spinner";
import { callSummary, formatClock, outcome, replyTables } from "./agentText";

export type CallRowProps = {
  entry: AgentHistoryEntry,
  open: boolean,
  showAgent: boolean,
  onToggle: () => void,
};

const APPROVAL_LABELS = {
  approved: "You approved it",
  rejected: "You rejected it",
  timedOut: "Nobody answered in time, so it was rejected",
  cancelled: "The agent stopped waiting before you answered",
};

const GRID_HEADER = 42;
const GRID_ROW = 26;
const GRID_MAX = 300;

function sqlHint(entry: AgentHistoryEntry): string {
  if (entry.approval?.answer === "approved") return "ran after you approved it";
  if (entry.approval || entry.outcome === "refused") return "never ran";
  return "ran in a read-only connection";
}

function CallDetail(props: { entry: AgentHistoryEntry }) {
  const [full] = createResource(
    () => props.entry,
    entry => safeInvoke("get_agent_history_entry", { id: entry.id, at: entry.at }),
  );
  const tables = createMemo(() => replyTables(full()?.reply));
  const replyJson = createMemo(() => {
    const reply = full()?.reply;
    return reply === undefined || reply === null || tables().length > 0 ? null : JSON.stringify(reply, null, 2);
  });
  const argumentsJson = createMemo(() => JSON.stringify(props.entry.arguments ?? {}, null, 2));
  const sql = createMemo(() => props.entry.statements.map(statement => `${statement};`).join("\n\n"));
  const hasSql = createMemo(() => props.entry.statements.length > 0);
  const failed = createMemo(() => props.entry.outcome !== "ok");

  const approval = createMemo(() => {
    const record = props.entry.approval;
    return record ? `${APPROVAL_LABELS[record.answer]}, ${formatDuration(record.answeredAt - record.requestedAt)} after it asked.` : null;
  });

  function gridHeight(rows: number): string {
    return `${Math.min(GRID_MAX, GRID_HEADER + Math.max(rows, 1) * GRID_ROW)}px`;
  }

  return (
    <div class={styles.Detail}>
      <Show when={props.entry.message}>
        <div class={styles.Message} classList={{ [styles.Failed]: failed(), [styles.Wide]: true }}>
          <CircleAlert size={14} />
          <span>{props.entry.message}</span>
        </div>
      </Show>

      <Show when={approval()}>
        <div class={`${styles.ApprovalLine} ${styles.Wide}`}>
          <ShieldCheck size={14} />
          <span>{approval()}</span>
        </div>
      </Show>

      <div class={styles.Block} classList={{ [styles.Wide]: !hasSql() }}>
        <div class={styles.Label}>Arguments</div>
        <div class={styles.Code}>
          <CodeView value={argumentsJson()} language="json" readOnly />
        </div>
      </div>

      <Show when={hasSql()}>
        <div class={styles.Block}>
          <div class={styles.Label}>SQL <span class={styles.Hint}>{sqlHint(props.entry)}</span></div>
          <div class={styles.Code}>
            <CodeView value={sql()} language="text" readOnly />
          </div>
        </div>
      </Show>

      <Show when={full.loading}>
        <div class={`${styles.Loading} ${styles.Wide}`}><Spinner size={14} /></div>
      </Show>
      <Show when={full.error}>
        <div class={styles.Message} classList={{ [styles.Failed]: true, [styles.Wide]: true }}>
          <CircleAlert size={14} />
          <span>The reply couldn't be read. {errorMessage(full.error)}</span>
        </div>
      </Show>
      <For each={tables()}>
        {table => (
          <div class={`${styles.Block} ${styles.Wide}`}>
            <div class={styles.Label}>
              Reply
              <span class={styles.Hint}>
                {plural(table.rows.length, "row")}{table.truncated ? ", cut short" : ""}, exactly what the agent got
              </span>
            </div>
            <div class={styles.Grid} style={{ height: gridHeight(table.rows.length) }}>
              <ResultGrid columns={table.columns} rows={table.rows} />
            </div>
          </div>
        )}
      </For>
      <Show when={replyJson()}>
        {json => (
          <div class={`${styles.Block} ${styles.Wide}`}>
            <div class={styles.Label}>Reply <span class={styles.Hint}>exactly what the agent got</span></div>
            <div class={`${styles.Code} ${styles.Tall}`}>
              <CodeView value={json()} language="json" readOnly />
            </div>
          </div>
        )}
      </Show>
    </div>
  );
}

function CallRow(props: CallRowProps) {
  const summary = createMemo(() => callSummary(props.entry));
  const result = createMemo(() => outcome(props.entry));

  return (
    <div class={styles.Call} classList={{ [styles.Open]: props.open }}>
      <button class={styles.CallRow} onClick={() => props.onToggle()}>
        {props.open ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
        <span class={styles.Time}>{formatClock(props.entry.at)}</span>
        <span class={styles.Tool}>{props.entry.tool ?? "call"}</span>
        <span class={styles.What}>
          <Show when={summary().code}>
            <code>{summary().code}</code>
          </Show>
          <Show when={summary().code && summary().text}>, </Show>
          {summary().text}
        </span>
        <Show when={props.showAgent && props.entry.agent}>
          <span class={styles.Agent}>{props.entry.agent}</span>
        </Show>
        <span class={styles.Duration}>{formatDuration(props.entry.durationMs)}</span>
        <span class={styles.Outcome} data-tone={result().tone}>{result().label}</span>
      </button>
      <Show when={props.open}>
        <CallDetail entry={props.entry} />
      </Show>
    </div>
  );
}

export default CallRow;
