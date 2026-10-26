import { createRoot } from "solid-js";
import { createStore, produce, reconcile } from "solid-js/store";
import { safeInvoke } from "@core/helpers/safeInvoke";
import { withConnectPrompts } from "@core/helpers/connectFlow";
import { generateId } from "@core/helpers";
import { databaseModule, withDatabaseModule } from "@core/databases";
import {
  CatalogColumn, ConnectionProfile, DatabaseInfo, DatabaseKind, ObjectInfo, ObjectKind, SavedQuery, SessionInfo,
} from "@core/types";
import useToastState from "./toastState";
import useSettingsState from "./settingsState";

export const HOME_TAB_ID = "home";

export type GlobalTab =
  | { id: typeof HOME_TAB_ID, kind: "home" }
  | { id: string, kind: "session", sessionId: string };

export type QueryTabState = {
  id: string,
  kind: "query",
  title: string,
  initialSql: string,
  runOnOpen?: boolean,
};

export type DatabaseTab =
  | QueryTabState
  | StructureTabState
  | { id: string, kind: "design", title: string, table: string | null };

export type StructureTabState = {
  id: string,
  kind: "structure",
  title: string,
  objectName: string,
  objectKind: ObjectKind,
  objectSignature: string | null,
};

export type DatabaseScope = {
  tabs: DatabaseTab[],
  activeTabId: string | null,
  queryCount: number,
};

export type Loadable<T> = {
  status: "idle" | "loading" | "ready" | "error",
  items: T,
  error: unknown,
};

export type SessionState = {
  info: SessionInfo,
  profile: ConnectionProfile,
  selectedDatabase: string | null,
  scopes: Record<string, DatabaseScope>,
  databases: Loadable<DatabaseInfo[]>,
  databaseSizes: Record<string, number>,
  objects: Record<string, Loadable<ObjectInfo[]>>,
  columns: Record<string, Loadable<CatalogColumn[]>>,
  expanded: Record<string, boolean>,
  switchingCatalog: boolean,
};

type WorkspaceStore = {
  globalTabs: GlobalTab[],
  activeGlobalTabId: string,
  sessions: Record<string, SessionState>,
  connecting: Record<string, boolean>,
};

function emptyScope(): DatabaseScope {
  return { tabs: [], activeTabId: null, queryCount: 0 };
}

const PERSIST_DELAY_MS = 400;
const PERSISTED_VERSION = 2;

type PersistedTab =
  | { kind: "query", title: string, sql: string }
  | { kind: "data", title: string, table: string }
  | { kind: "structure", title: string, objectName: string, objectKind: ObjectKind, objectSignature?: string | null }
  | { kind: "design", title: string, table: string | null };

type PersistedScope = { tabs: PersistedTab[], activeIndex: number, queryCount: number };

type PersistedSession = {
  selectedDatabase: string | null,
  expanded: string[],
  scopes: Record<string, PersistedScope>,
};

// One entry per database for servers whose sessions switch between databases, keyed "" otherwise.
type PersistedConnection = {
  version: number,
  catalogs: Record<string, PersistedSession>,
};

function isPersistedSession(value: unknown): value is PersistedSession {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Partial<PersistedSession>;
  return typeof candidate.scopes === "object"
    && candidate.scopes !== null
    && Array.isArray(candidate.expanded);
}

function readPersisted(value: unknown): PersistedConnection {
  const empty: PersistedConnection = { version: PERSISTED_VERSION, catalogs: {} };
  if (typeof value !== "object" || value === null) return empty;
  const candidate = value as { version?: unknown, catalogs?: unknown };

  // Version 1 stored a single session, from before servers had more than one database per connection.
  if (candidate.version === 1) {
    return isPersistedSession(value) ? { version: PERSISTED_VERSION, catalogs: { "": value } } : empty;
  }
  if (candidate.version !== PERSISTED_VERSION || typeof candidate.catalogs !== "object" || candidate.catalogs === null) {
    return empty;
  }

  const catalogs: Record<string, PersistedSession> = {};
  for (const [catalog, session] of Object.entries(candidate.catalogs)) {
    if (isPersistedSession(session)) catalogs[catalog] = session;
  }
  return { version: PERSISTED_VERSION, catalogs };
}

function catalogKey(info: SessionInfo): string {
  return info.catalog ?? "";
}

function sameSql(a: string, b: string): boolean {
  return a.trim().replace(/\s+/g, " ") === b.trim().replace(/\s+/g, " ");
}

function restoreTab(saved: PersistedTab, selectRowsSql: (table: string) => string): DatabaseTab | null {
  const id = generateId();
  switch (saved.kind) {
    case "query":
      return { id, kind: "query", title: saved.title, initialSql: saved.sql };
    case "data":
      return { id, kind: "query", title: saved.title, initialSql: selectRowsSql(saved.table) };
    case "structure":
      return {
        id,
        kind: "structure",
        title: saved.title,
        objectName: saved.objectName,
        objectKind: saved.objectKind,
        objectSignature: saved.objectSignature ?? null,
      };
    case "design":
      return { id, kind: "design", title: saved.title, table: saved.table };
    default:
      return null;
  }
}

function useWorkspaceStateInner() {
  const { pushError, pushToast } = useToastState;
  const [store, setStore] = createStore<WorkspaceStore>({
    globalTabs: [{ id: HOME_TAB_ID, kind: "home" }],
    activeGlobalTabId: HOME_TAB_ID,
    sessions: {},
    connecting: {},
  });

  // Live editor text per query tab. Kept out of the store so typing doesn't re-render the tab bar.
  const queryTexts = new Map<string, string>();
  const persistTimers = new Map<string, ReturnType<typeof setTimeout>>();
  const persistedConnections = new Map<string, PersistedConnection>();

  function queryText(tab: QueryTabState): string {
    return queryTexts.get(tab.id) ?? tab.initialSql;
  }

  function snapshot(session: SessionState): PersistedSession {
    const scopes: Record<string, PersistedScope> = {};
    for (const [database, scope] of Object.entries(session.scopes)) {
      const tabs: PersistedTab[] = scope.tabs.map(tab => {
        switch (tab.kind) {
          case "query":
            return { kind: "query", title: tab.title, sql: queryText(tab) };
          case "structure":
            return {
              kind: "structure",
              title: tab.title,
              objectName: tab.objectName,
              objectKind: tab.objectKind,
              objectSignature: tab.objectSignature,
            };
          case "design":
            return { kind: "design", title: tab.title, table: tab.table };
        }
      });
      scopes[database] = {
        tabs,
        activeIndex: scope.tabs.findIndex(tab => tab.id === scope.activeTabId),
        queryCount: scope.queryCount,
      };
    }

    return {
      selectedDatabase: session.selectedDatabase,
      expanded: Object.entries(session.expanded).filter(([, open]) => open).map(([database]) => database),
      scopes,
    };
  }

  async function loadPersisted(connectionId: string): Promise<PersistedConnection> {
    const cached = persistedConnections.get(connectionId);
    if (cached) return cached;

    const saved = await safeInvoke("get_tab_state", { connectionId }).catch(() => null);
    const persisted = readPersisted(saved);
    persistedConnections.set(connectionId, persisted);
    return persisted;
  }

  function persistNow(sessionId: string) {
    clearTimeout(persistTimers.get(sessionId));
    persistTimers.delete(sessionId);
    const session = store.sessions[sessionId];
    if (!session) return;

    const persisted = persistedConnections.get(session.profile.id) ?? { version: PERSISTED_VERSION, catalogs: {} };
    persisted.catalogs[catalogKey(session.info)] = snapshot(session);
    persistedConnections.set(session.profile.id, persisted);
    safeInvoke("set_tab_state", { connectionId: session.profile.id, state: persisted })
      .catch(error => pushError(error));
  }

  function schedulePersist(sessionId: string) {
    clearTimeout(persistTimers.get(sessionId));
    persistTimers.set(sessionId, setTimeout(() => persistNow(sessionId), PERSIST_DELAY_MS));
  }

  function setQueryText(sessionId: string, tabId: string, text: string) {
    queryTexts.set(tabId, text);
    schedulePersist(sessionId);
  }

  function restoredScopes(saved: PersistedSession, kind: DatabaseKind): Record<string, DatabaseScope> {
    const { selectRowsSql } = databaseModule(kind);
    const scopes: Record<string, DatabaseScope> = {};
    for (const [database, scope] of Object.entries(saved.scopes)) {
      const tabs = (Array.isArray(scope.tabs) ? scope.tabs : [])
        .map(tab => restoreTab(tab, selectRowsSql))
        .filter((tab): tab is DatabaseTab => tab !== null);
      for (const tab of tabs) {
        if (tab.kind === "query") queryTexts.set(tab.id, tab.initialSql);
      }
      scopes[database] = {
        tabs,
        activeTabId: tabs[scope.activeIndex]?.id ?? tabs[0]?.id ?? null,
        queryCount: scope.queryCount ?? 0,
      };
    }
    return scopes;
  }

  function activateGlobalTab(id: string) {
    setStore("activeGlobalTabId", id);
  }

  function sessionState(info: SessionInfo, profile: ConnectionProfile, saved: PersistedSession | undefined): SessionState {
    const defaultDatabase = withDatabaseModule(profile, (module, options) => module.defaultDatabase(options));
    const selectedDatabase = saved ? saved.selectedDatabase : defaultDatabase;
    const expanded: Record<string, boolean> = saved
      ? Object.fromEntries(saved.expanded.map(database => [database, true]))
      : defaultDatabase ? { [defaultDatabase]: true } : {};
    const scopes = saved ? restoredScopes(saved, info.kind) : {};
    if (selectedDatabase) scopes[selectedDatabase] ??= emptyScope();

    return {
      info,
      profile,
      selectedDatabase,
      scopes,
      databases: { status: "idle", items: [], error: null },
      databaseSizes: {},
      objects: {},
      columns: {},
      expanded,
      switchingCatalog: false,
    };
  }

  async function loadTree(sessionId: string) {
    await loadDatabases(sessionId);
    const session = store.sessions[sessionId];
    if (!session) return;
    for (const [database, open] of Object.entries(session.expanded)) {
      if (open) loadObjects(sessionId, database);
    }
  }

  async function openConnection(profile: ConnectionProfile) {
    if (store.connecting[profile.id]) return;
    setStore("connecting", profile.id, true);

    try {
      const info = await withConnectPrompts(
        { connectionId: profile.id, connectionName: profile.name },
        secrets => safeInvoke("open_session", { connectionId: profile.id, secrets }),
      );
      if (!info) return;

      const persisted = await loadPersisted(profile.id);
      const state = sessionState(info, profile, persisted.catalogs[catalogKey(info)]);

      setStore(produce(workspace => {
        workspace.sessions[info.id] = state;
        workspace.globalTabs.push({ id: info.id, kind: "session", sessionId: info.id });
        workspace.activeGlobalTabId = info.id;
      }));

      await loadTree(info.id);
    } catch (error) {
      pushError(error);
    } finally {
      setStore("connecting", profile.id, false);
    }
  }

  // Tabs are saved per database, so leaving one keeps its tabs for when the user comes back.
  async function switchCatalog(sessionId: string, catalog: string) {
    const session = store.sessions[sessionId];
    if (!session || session.switchingCatalog || session.info.catalog === catalog) return;

    persistNow(sessionId);
    setStore("sessions", sessionId, "switchingCatalog", true);

    try {
      const info = await safeInvoke("switch_catalog", { sessionId, catalog });
      const current = store.sessions[sessionId];
      if (!current) return;

      for (const scope of Object.values(current.scopes)) {
        for (const tab of scope.tabs) queryTexts.delete(tab.id);
      }
      const persisted = await loadPersisted(current.profile.id);
      const next = sessionState(info, current.profile, persisted.catalogs[catalogKey(info)]);
      setStore(produce(workspace => {
        workspace.sessions[sessionId] = next;
      }));

      await loadTree(sessionId);
    } catch (error) {
      if (store.sessions[sessionId]) setStore("sessions", sessionId, "switchingCatalog", false);
      pushError(error);
    }
  }

  async function closeSession(sessionId: string) {
    setStore(produce(state => {
      const index = state.globalTabs.findIndex(tab => tab.id === sessionId);
      if (index < 0) return;

      state.globalTabs.splice(index, 1);
      if (state.activeGlobalTabId === sessionId) {
        state.activeGlobalTabId = state.globalTabs[Math.max(0, index - 1)]?.id ?? HOME_TAB_ID;
      }
      delete state.sessions[sessionId];
    }));

    try {
      await safeInvoke("close_session", { sessionId });
    } catch (error) {
      pushError(error);
    }
  }

  async function loadDatabases(sessionId: string) {
    if (!store.sessions[sessionId]) return;
    setStore("sessions", sessionId, "databases", { status: "loading", error: null });

    try {
      const items = await safeInvoke("list_databases", { sessionId });
      if (!store.sessions[sessionId]) return;
      setStore("sessions", sessionId, "databases", { status: "ready", items, error: null });
      loadDatabaseSizes(sessionId);
    } catch (error) {
      if (!store.sessions[sessionId]) return;
      setStore("sessions", sessionId, "databases", { status: "error", error });
    }
  }

  async function loadDatabaseSizes(sessionId: string) {
    try {
      const sizes = await safeInvoke("get_database_sizes", { sessionId });
      if (!store.sessions[sessionId]) return;
      setStore("sessions", sessionId, "databaseSizes", reconcile(Object.fromEntries(sizes.map(size => [size.name, size.sizeBytes]))));
    } catch (error) {
      if (store.sessions[sessionId]) pushError(error);
    }
  }

  async function loadObjects(sessionId: string, database: string) {
    if (!store.sessions[sessionId]) return;
    setStore("sessions", sessionId, "objects", database, (current): Loadable<ObjectInfo[]> => ({
      status: "loading",
      items: current?.items ?? [],
      error: null,
    }));

    try {
      const items = await safeInvoke("list_objects", { sessionId, database });
      if (!store.sessions[sessionId]) return;
      setStore("sessions", sessionId, "objects", database, { status: "ready", items, error: null });
    } catch (error) {
      if (!store.sessions[sessionId]) return;
      setStore("sessions", sessionId, "objects", database, { status: "error", error });
    }
  }

  async function loadColumns(sessionId: string, database: string) {
    if (!store.sessions[sessionId]) return;
    setStore("sessions", sessionId, "columns", database, (current): Loadable<CatalogColumn[]> => ({
      status: "loading",
      items: current?.items ?? [],
      error: null,
    }));

    try {
      const items = await safeInvoke("list_columns", { sessionId, database });
      if (!store.sessions[sessionId]) return;
      setStore("sessions", sessionId, "columns", database, { status: "ready", items, error: null });
    } catch (error) {
      if (!store.sessions[sessionId]) return;
      setStore("sessions", sessionId, "columns", database, { status: "error", error });
    }
  }

  function ensureCatalog(sessionId: string, database: string) {
    const session = store.sessions[sessionId];
    if (!session) return;
    if (!session.objects[database]) loadObjects(sessionId, database);
    if (!session.columns[database]) loadColumns(sessionId, database);
  }

  // After DDL the object list and column catalog are stale.
  async function refreshCatalog(sessionId: string, database: string) {
    await Promise.all([loadObjects(sessionId, database), loadColumns(sessionId, database), loadDatabaseSizes(sessionId)]);
  }

  function toggleExpanded(sessionId: string, database: string) {
    const session = store.sessions[sessionId];
    if (!session) return;

    const expand = !session.expanded[database];
    setStore("sessions", sessionId, "expanded", database, expand);
    if (expand && !session.objects[database]) loadObjects(sessionId, database);
    schedulePersist(sessionId);
  }

  function selectDatabase(sessionId: string, database: string) {
    setStore("sessions", sessionId, produce(session => {
      session.selectedDatabase = database;
      session.scopes[database] ??= emptyScope();
    }));
    schedulePersist(sessionId);
  }

  function openTab(sessionId: string, database: string, tab: DatabaseTab) {
    setStore("sessions", sessionId, produce(session => {
      session.selectedDatabase = database;
      const scope = (session.scopes[database] ??= emptyScope());
      scope.tabs.push(tab);
      scope.activeTabId = tab.id;
    }));
    schedulePersist(sessionId);
  }

  function nextQueryTitle(sessionId: string, database: string): string {
    const count = (store.sessions[sessionId]?.scopes[database]?.queryCount ?? 0) + 1;
    setStore("sessions", sessionId, "scopes", database, scope => ({ ...(scope ?? emptyScope()), queryCount: count }));
    return `Query ${count}`;
  }

  function newQueryTab(sessionId: string, database: string, initialSql = "") {
    if (!store.sessions[sessionId]) return;
    const title = nextQueryTitle(sessionId, database);
    openTab(sessionId, database, { id: generateId(), kind: "query", title, initialSql });
  }

  function focusExisting(sessionId: string, database: string, matches: (tab: DatabaseTab) => boolean): boolean {
    const existing = store.sessions[sessionId]?.scopes[database]?.tabs.find(matches);
    if (!existing) return false;

    selectDatabase(sessionId, database);
    setStore("sessions", sessionId, "scopes", database, "activeTabId", existing.id);
    schedulePersist(sessionId);
    return true;
  }

  function openSavedQuery(sessionId: string, query: SavedQuery) {
    const session = store.sessions[sessionId];
    if (!session) return;

    const known = session.databases.items.some(database => database.name === query.database);
    const database = known && query.database ? query.database : session.selectedDatabase;
    if (!database) {
      const namespace = databaseModule(session.info.kind).namespace.singular;
      pushToast(`Pick a ${namespace} first, the query will open there`, "info");
      return;
    }

    openTab(sessionId, database, { id: generateId(), kind: "query", title: query.name, initialSql: query.sql });
  }

  // A tab whose text is still the generated SELECT is reused, edited ones are left alone.
  function queryTable(sessionId: string, database: string, table: string) {
    const session = store.sessions[sessionId];
    if (!session) return;

    const sql = databaseModule(session.info.kind).selectRowsSql(table);
    if (focusExisting(sessionId, database, tab => tab.kind === "query" && sameSql(queryText(tab), sql))) return;
    openTab(sessionId, database, {
      id: generateId(),
      kind: "query",
      title: table,
      initialSql: sql,
      runOnOpen: useSettingsState.settings.runTableQueryOnOpen,
    });
  }

  function clearRunOnOpen(sessionId: string, database: string, tabId: string) {
    setStore("sessions", sessionId, "scopes", database, "tabs", tab => tab.id === tabId, produce(tab => {
      if (tab.kind === "query") delete tab.runOnOpen;
    }));
  }

  function openStructure(sessionId: string, database: string, object: ObjectInfo) {
    const matches = (tab: DatabaseTab) => tab.kind === "structure"
      && tab.objectName === object.name
      && tab.objectKind === object.kind
      && tab.objectSignature === object.signature;
    if (focusExisting(sessionId, database, matches)) return;
    openTab(sessionId, database, {
      id: generateId(),
      kind: "structure",
      title: object.name,
      objectName: object.name,
      objectKind: object.kind,
      objectSignature: object.signature,
    });
  }

  function openDesigner(sessionId: string, database: string, table: string | null) {
    if (table && focusExisting(sessionId, database, tab => tab.kind === "design" && tab.table === table)) return;
    openTab(sessionId, database, { id: generateId(), kind: "design", title: table ?? "New table", table });
  }

  function updateTab(sessionId: string, database: string, tabId: string, change: (tab: DatabaseTab) => void) {
    setStore("sessions", sessionId, "scopes", database, "tabs", tab => tab.id === tabId, produce(change));
    schedulePersist(sessionId);
  }

  function activateTab(sessionId: string, database: string, tabId: string) {
    setStore("sessions", sessionId, "scopes", database, "activeTabId", tabId);
    schedulePersist(sessionId);
  }

  function closeTab(sessionId: string, database: string, tabId: string) {
    setStore("sessions", sessionId, "scopes", database, produce(scope => {
      const index = scope.tabs.findIndex(tab => tab.id === tabId);
      if (index < 0) return;

      scope.tabs.splice(index, 1);
      if (scope.activeTabId === tabId) {
        scope.activeTabId = scope.tabs[Math.min(index, scope.tabs.length - 1)]?.id ?? null;
      }
    }));
    queryTexts.delete(tabId);
    schedulePersist(sessionId);
  }

  function closeOtherTabs(sessionId: string, database: string, keepId: string) {
    const closing = store.sessions[sessionId]?.scopes[database]?.tabs.filter(tab => tab.id !== keepId) ?? [];
    setStore("sessions", sessionId, "scopes", database, produce(scope => {
      scope.tabs = scope.tabs.filter(tab => tab.id === keepId);
      scope.activeTabId = keepId;
    }));
    for (const tab of closing) queryTexts.delete(tab.id);
    schedulePersist(sessionId);
  }

  return {
    store,
    queryText,
    setQueryText,
    activateGlobalTab,
    openConnection,
    switchCatalog,
    closeSession,
    loadDatabases,
    loadObjects,
    loadColumns,
    ensureCatalog,
    refreshCatalog,
    toggleExpanded,
    selectDatabase,
    newQueryTab,
    openSavedQuery,
    queryTable,
    clearRunOnOpen,
    openStructure,
    openDesigner,
    updateTab,
    activateTab,
    closeTab,
    closeOtherTabs,
  };
}

const useWorkspaceState = createRoot(useWorkspaceStateInner);
export default useWorkspaceState;
