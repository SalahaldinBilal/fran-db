import { formatDialect } from "sql-formatter";

type FormatDialect = NonNullable<Parameters<typeof formatDialect>[1]>["dialect"];

export function sqlFormatter(dialect: FormatDialect): (sql: string) => string {
  return sql => formatDialect(sql, { dialect, keywordCase: "upper", tabWidth: 2, logicalOperatorNewline: "after" });
}
