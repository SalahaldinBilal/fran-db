import { createRoot, createSignal } from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import { safeInvoke } from "@core/helpers/safeInvoke";
import useToastState from "./toastState";

export type CellEditMode = "viewer" | "inline";

export type LetterCase = "upper" | "lower" | "preserve";
export type FormatIndent = "spaces2" | "spaces4" | "tabs";
export type OperatorPlacement = "before" | "after";

export type FormatSettings = {
  keywordCase: LetterCase,
  functionCase: LetterCase,
  dataTypeCase: LetterCase,
  indent: FormatIndent,
  logicalOperatorNewline: OperatorPlacement,
  expressionWidth: number,
  linesBetweenQueries: number,
};

export type AppSettings = {
  cellEditMode: CellEditMode,
  runTableQueryOnOpen: boolean,
  defaultRowLimit: number,
  colorFill: boolean,
  format: FormatSettings,
};

const CELL_EDIT_MODES: readonly CellEditMode[] = ["viewer", "inline"];
const LETTER_CASES: readonly LetterCase[] = ["upper", "lower", "preserve"];
const FORMAT_INDENTS: readonly FormatIndent[] = ["spaces2", "spaces4", "tabs"];
const OPERATOR_PLACEMENTS: readonly OperatorPlacement[] = ["before", "after"];
export const EXPRESSION_WIDTHS: readonly number[] = [40, 50, 60, 80, 100, 120];
export const LINES_BETWEEN_QUERIES: readonly number[] = [0, 1, 2];

const DEFAULT_FORMAT: FormatSettings = {
  keywordCase: "upper",
  functionCase: "preserve",
  dataTypeCase: "preserve",
  indent: "spaces2",
  logicalOperatorNewline: "after",
  expressionWidth: 50,
  linesBetweenQueries: 1,
};

export const ROW_LIMITS: readonly { value: number, label: string }[] = [
  { value: 100, label: "Limit 100" },
  { value: 500, label: "Limit 500" },
  { value: 1000, label: "Limit 1,000" },
  { value: 5000, label: "Limit 5,000" },
  { value: 10000, label: "Limit 10,000" },
  { value: 0, label: "No limit" },
];

export function isRowLimit(value: unknown): value is number {
  return ROW_LIMITS.some(limit => limit.value === value);
}

export const DEFAULT_SETTINGS: AppSettings = {
  cellEditMode: "viewer",
  runTableQueryOnOpen: true,
  defaultRowLimit: 1000,
  colorFill: true,
  format: DEFAULT_FORMAT,
};

function oneOf<T>(value: unknown, allowed: readonly T[], fallback: T): T {
  return allowed.includes(value as T) ? value as T : fallback;
}

function formatFromStored(stored: unknown): FormatSettings {
  const values = (stored && typeof stored === "object" ? stored : {}) as Partial<Record<keyof FormatSettings, unknown>>;
  return {
    keywordCase: oneOf(values.keywordCase, LETTER_CASES, DEFAULT_FORMAT.keywordCase),
    functionCase: oneOf(values.functionCase, LETTER_CASES, DEFAULT_FORMAT.functionCase),
    dataTypeCase: oneOf(values.dataTypeCase, LETTER_CASES, DEFAULT_FORMAT.dataTypeCase),
    indent: oneOf(values.indent, FORMAT_INDENTS, DEFAULT_FORMAT.indent),
    logicalOperatorNewline: oneOf(values.logicalOperatorNewline, OPERATOR_PLACEMENTS, DEFAULT_FORMAT.logicalOperatorNewline),
    expressionWidth: oneOf(values.expressionWidth, EXPRESSION_WIDTHS, DEFAULT_FORMAT.expressionWidth),
    linesBetweenQueries: oneOf(values.linesBetweenQueries, LINES_BETWEEN_QUERIES, DEFAULT_FORMAT.linesBetweenQueries),
  };
}

// Keys missing from an older file, or holding values this version doesn't know, take the default.
function fromStored(stored: unknown): AppSettings {
  const values = (stored && typeof stored === "object" ? stored : {}) as Partial<Record<keyof AppSettings, unknown>>;
  return {
    cellEditMode: CELL_EDIT_MODES.includes(values.cellEditMode as CellEditMode)
      ? values.cellEditMode as CellEditMode
      : DEFAULT_SETTINGS.cellEditMode,
    runTableQueryOnOpen: typeof values.runTableQueryOnOpen === "boolean"
      ? values.runTableQueryOnOpen
      : DEFAULT_SETTINGS.runTableQueryOnOpen,
    defaultRowLimit: isRowLimit(values.defaultRowLimit) ? values.defaultRowLimit : DEFAULT_SETTINGS.defaultRowLimit,
    colorFill: typeof values.colorFill === "boolean" ? values.colorFill : DEFAULT_SETTINGS.colorFill,
    format: formatFromStored(values.format),
  };
}

function useSettingsStateInner() {
  const { pushError } = useToastState;
  const [settings, setSettings] = createStore<AppSettings>({ ...DEFAULT_SETTINGS });
  const [loadError, setLoadError] = createSignal<unknown>(null);

  async function load() {
    try {
      setSettings(reconcile(fromStored(await safeInvoke("get_settings"))));
      setLoadError(null);
    } catch (error) {
      setLoadError(error);
    }
  }

  async function update(patch: Partial<AppSettings>) {
    const previous: AppSettings = { ...settings, format: { ...settings.format } };
    const next = { ...previous, ...patch };
    setSettings(reconcile(next));
    try {
      await safeInvoke("set_settings", { settings: next });
    } catch (error) {
      setSettings(reconcile(previous));
      pushError(error);
    }
  }

  function updateFormat(patch: Partial<FormatSettings>) {
    return update({ format: { ...settings.format, ...patch } });
  }

  return { settings, loadError, load, update, updateFormat };
}

const useSettingsState = createRoot(useSettingsStateInner);
export default useSettingsState;
