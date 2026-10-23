import { BatchOutcome } from "@core/types";
import { plural } from "./index";

export function batchFailureMessage(outcome: BatchOutcome): string {
  const message = outcome.error?.message ?? "Unknown error";
  if (outcome.failedIndex === null) return message;

  const where = `Statement ${outcome.failedIndex + 1} failed: ${message}`;
  if (outcome.rolledBack) return `${where}. Nothing was changed.`;
  if (outcome.executed === 0) return where;
  return `${where}. ${plural(outcome.executed, "statement")} before it already ran.`;
}
