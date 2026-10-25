import { DatabaseModule } from "../types";
import sqliteModule from "../sqlite";
import D1ConnectionForm from "./D1ConnectionForm";

const d1Module: DatabaseModule<"d1"> = {
  ...sqliteModule,
  kind: "d1",
  label: "Cloudflare D1",
  isSystemTable: name => sqliteModule.isSystemTable(name) || name.startsWith("_cf_"),
  catalogLabel: "Database",
  defaultOptions: () => ({
    accountId: "",
    database: "",
    timeoutSecs: 30,
  }),
  ConnectionForm: D1ConnectionForm,
  describeTarget: options => `${options.database.trim() || "(no database)"} on D1`,
  suggestName: options => options.database.trim() || "Cloudflare D1",
  methodLabel: () => "HTTP API",
  sshTarget: () => null,
  defaultDatabase: () => "main",
};

export default d1Module;
