import { CalendarClock, Eye, Layers, ListOrdered, LucideIcon, Sigma, Table2, Workflow, Zap } from "lucide-solid";
import { ObjectKind } from "@core/types";

export type ObjectKindInfo = {
  kind: ObjectKind,
  label: string,
  singular: string,
  icon: LucideIcon,
  hasRows: boolean,
  hasColumns: boolean,
};

export const OBJECT_KINDS: readonly ObjectKindInfo[] = [
  { kind: "table", label: "Tables", singular: "table", icon: Table2, hasRows: true, hasColumns: true },
  { kind: "view", label: "Views", singular: "view", icon: Eye, hasRows: true, hasColumns: true },
  {
    kind: "materializedView",
    label: "Materialized views",
    singular: "materialized view",
    icon: Layers,
    hasRows: true,
    hasColumns: true,
  },
  { kind: "sequence", label: "Sequences", singular: "sequence", icon: ListOrdered, hasRows: false, hasColumns: false },
  { kind: "procedure", label: "Procedures", singular: "procedure", icon: Workflow, hasRows: false, hasColumns: false },
  { kind: "function", label: "Functions", singular: "function", icon: Sigma, hasRows: false, hasColumns: false },
  { kind: "trigger", label: "Triggers", singular: "trigger", icon: Zap, hasRows: false, hasColumns: false },
  { kind: "event", label: "Events", singular: "event", icon: CalendarClock, hasRows: false, hasColumns: false },
];

export function objectKindInfo(kind: ObjectKind): ObjectKindInfo {
  return OBJECT_KINDS.find(info => info.kind === kind) ?? OBJECT_KINDS[0];
}
