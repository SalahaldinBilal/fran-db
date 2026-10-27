import { formatDialect, sql as standardSql } from "sql-formatter";
import type { FormatSettings } from "@core/states/settingsState";

type FormatDialect = NonNullable<Parameters<typeof formatDialect>[1]>["dialect"];

export type SqlFormatter = (sql: string, settings: FormatSettings) => string;

export function sqlFormatter(dialect: FormatDialect): SqlFormatter {
  return (sql, settings) => formatDialect(sql, {
    dialect,
    keywordCase: settings.keywordCase,
    functionCase: settings.functionCase,
    dataTypeCase: settings.dataTypeCase,
    tabWidth: settings.indent === "spaces4" ? 4 : 2,
    useTabs: settings.indent === "tabs",
    logicalOperatorNewline: settings.logicalOperatorNewline,
    expressionWidth: settings.expressionWidth,
    linesBetweenQueries: settings.linesBetweenQueries,
  });
}

export const formatStandardSql = sqlFormatter(standardSql);
