export type TextRange = { start: number, end: number };

// The statement containing the cursor, else the last one before it, else the first.
export function statementAt(ranges: readonly TextRange[], cursor: number): number {
  const containing = ranges.findIndex(range => cursor >= range.start && cursor <= range.end);
  if (containing !== -1) return containing;
  for (let index = ranges.length - 1; index >= 0; index--) {
    if (ranges[index].end <= cursor) return index;
  }
  return ranges.length > 0 ? 0 : -1;
}
