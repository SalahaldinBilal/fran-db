import styles from "./AgentApprovalModal.module.scss";
import { For, Match, Show, Switch, createMemo, createSignal, onCleanup } from "solid-js";
import { ArrowRight, CircleAlert, ShieldCheck, TriangleAlert } from "lucide-solid";
import { AgentApproval, PreviewRow } from "@core/types";
import { databaseModule } from "@core/databases";
import { errorMessage } from "@core/helpers/errors";
import { plural } from "@core/helpers";
import useAgentState from "@core/states/agentState";
import AgentMark from "@core/pages/Agents/AgentMark";
import { formatClock, tableOf, where } from "@core/pages/Agents/agentText";
import Modal from "../Modal/Modal";
import Button from "../Button/Button";
import SegmentedControl, { SegmentedOption } from "../SegmentedControl/SegmentedControl";
import SqlEditor from "../SqlEditor/SqlEditor";
import CodeView from "../CodeView/CodeView";
import Spinner from "../Spinner/Spinner";
import { safetyNote } from "../ReviewSqlModal/ReviewSqlModal";

type Pane = "changes" | "sql" | "arguments";

const APPROVAL_TIMEOUT_MS = 5 * 60_000;

function keyText(row: PreviewRow): string {
  return row.key.map(part => `${part.column} = ${part.value ?? "NULL"}`).join(", ");
}

function rowTitle(row: PreviewRow): string {
  const missing = row.found === false ? ". No row has this key, so nothing changes" : "";
  switch (row.action) {
    case "insert": return "New row";
    case "update": return `Row ${keyText(row)}${missing}`;
    case "delete": return `Delete row ${keyText(row)}${missing}`;
  }
}

function Value(props: { value: string | null, class?: string }) {
  return (
    <Show when={props.value !== null} fallback={<span class={styles.Null}>NULL</span>}>
      <span class={props.class}>{props.value}</span>
    </Show>
  );
}

function ChangesTable(props: { approval: AgentApproval }) {
  const preview = createMemo(() => props.approval.preview);

  return (
    <div class={styles.Grid}>
      <table>
        <thead>
          <tr>
            <th>Column</th>
            <th>Now</th>
            <th class={styles.Arrow} />
            <th>After</th>
          </tr>
        </thead>
        <tbody>
          <For each={preview()?.rows ?? []}>
            {row => (
              <>
                <tr class={styles.RowHead}>
                  <td colSpan={4} classList={{ [styles.Missing]: row.found === false }}>{rowTitle(row)}</td>
                </tr>
                <For each={row.cells}>
                  {cell => (
                    <tr>
                      <td class={styles.Column}>{cell.column}</td>
                      <td class={styles.Old}>
                        <Switch>
                          <Match when={row.action === "insert"}>{null}</Match>
                          <Match when={row.found === null}><span class={styles.Unknown}>couldn't be read</span></Match>
                          <Match when={true}><Value value={cell.before} /></Match>
                        </Switch>
                      </td>
                      <td class={styles.Arrow}><ArrowRight size={12} /></td>
                      <td classList={{ [styles.New]: row.action !== "delete" }}>
                        <Show when={row.action !== "delete"} fallback={<span class={styles.Unknown}>removed</span>}>
                          <Value value={cell.after} />
                        </Show>
                      </td>
                    </tr>
                  )}
                </For>
              </>
            )}
          </For>
          <Show when={(preview()?.more ?? 0) > 0}>
            <tr class={styles.RowHead}>
              <td colSpan={4}>And {plural(preview()?.more ?? 0, "more row")}, all in the SQL tab</td>
            </tr>
          </Show>
        </tbody>
      </table>
    </div>
  );
}

function ReviewDialog(props: { approval: AgentApproval }) {
  const agent = useAgentState;
  const [pane, setPane] = createSignal<Pane>(props.approval.preview ? "changes" : "sql");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [now, setNow] = createSignal(Date.now());

  const timer = setInterval(() => setNow(Date.now()), 1000);
  onCleanup(() => clearInterval(timer));

  const dialect = createMemo(() => databaseModule(props.approval.kind).sqlDialect);
  const script = createMemo(() => props.approval.statements.map(statement => `${statement};`).join("\n\n"));
  const argumentsJson = createMemo(() => JSON.stringify(props.approval.arguments ?? {}, null, 2));
  const table = createMemo(() => props.approval.preview?.table ?? tableOf(props.approval.arguments));
  const note = createMemo(() => safetyNote(props.approval.safety, props.approval.statements.length));

  const panes = createMemo<SegmentedOption<Pane>[]>(() => {
    const rows = props.approval.preview ? props.approval.preview.rows.length + props.approval.preview.more : null;
    return [
      ...(rows === null ? [] : [{ value: "changes" as const, label: "Changes", count: rows }]),
      { value: "sql", label: "SQL", count: props.approval.statements.length },
      { value: "arguments", label: "Arguments" },
    ];
  });

  const countdown = createMemo(() => {
    const left = Math.max(0, props.approval.requestedAt + APPROVAL_TIMEOUT_MS - now());
    const seconds = Math.ceil(left / 1000);
    if (seconds === 0) return "Rejecting now, nobody answered in time";
    return `Rejected automatically in ${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")} if you don't answer`;
  });

  async function respond(approved: boolean) {
    setBusy(true);
    setError(null);
    try {
      await agent.answer(props.approval.id, approved);
    } catch (failure) {
      setError(errorMessage(failure));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal
      show
      onHide={() => agent.closeReview()}
      title={`Review a change from ${props.approval.agent}`}
      subtitle="Nothing runs until you approve it. The agent is waiting for your answer."
      width={820}
      footer={
        <>
          <span class={styles.Timer}>{countdown()}</span>
          <Button variant="subtle" disabled={busy()} onClick={() => respond(false)}>Reject</Button>
          <Button variant="primary" disabled={busy()} onClick={() => respond(true)}>
            <Show when={busy()}><Spinner size={12} /></Show>
            Approve
          </Button>
        </>
      }
    >
      <div class={styles.Body}>
        <div class={styles.Context}>
          <AgentMark agent={props.approval.agent} />
          <dl class={styles.Facts}>
            <div><dt>Connection</dt><dd>{where(props.approval.connectionName, props.approval.database)}</dd></div>
            <div><dt>Tool</dt><dd><code>{props.approval.tool}</code></dd></div>
            <Show when={table()}>
              <div><dt>Table</dt><dd><code>{table()}</code></dd></div>
            </Show>
            <div><dt>Asked</dt><dd>{formatClock(props.approval.requestedAt)}</dd></div>
          </dl>
        </div>

        <div class={styles.Panes}>
          <SegmentedControl<Pane> value={pane()} options={panes()} onChange={setPane} />
        </div>

        <Switch>
          <Match when={pane() === "changes"}>
            <ChangesTable approval={props.approval} />
          </Match>
          <Match when={pane() === "sql"}>
            <div class={styles.Code}>
              <SqlEditor initialValue={script()} dialect={dialect()} readOnly wrap />
            </div>
          </Match>
          <Match when={pane() === "arguments"}>
            <div class={`${styles.Code} ${styles.Json}`}>
              <CodeView value={argumentsJson()} language="json" readOnly />
            </div>
          </Match>
        </Switch>

        <div class={styles.Safety} classList={{ [styles.Unsafe]: !note().safe }}>
          {note().safe ? <ShieldCheck size={14} /> : <TriangleAlert size={14} />}
          <span>{note().text}</span>
        </div>
        <Show when={error()}>
          <div class={styles.Error}>
            <CircleAlert size={14} />
            <span>{error()}</span>
          </div>
        </Show>
      </div>
    </Modal>
  );
}

function AgentApprovalModal() {
  const agent = useAgentState;

  return (
    <Show when={agent.reviewing()} keyed>
      {approval => <ReviewDialog approval={approval} />}
    </Show>
  );
}

export default AgentApprovalModal;
