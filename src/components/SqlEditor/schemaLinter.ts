import { syntaxTree } from "@codemirror/language";
import { Diagnostic, linter } from "@codemirror/lint";
import { EditorState } from "@codemirror/state";

type SyntaxNode = ReturnType<typeof syntaxTree>["topNode"];

// Lowercased names. A table maps to its columns, or null when its columns aren't known.
export type SchemaCatalog = {
  database: string,
  databases: Set<string>,
  tables: Map<string, Map<string, Set<string> | null>>,
  // Where unqualified tables are looked up, in order.
  searchPath: string[],
  // What `databases` holds, for messages: "database" or "schema".
  namespace: string,
  // Built-in tables that resolve without being in the catalog, such as Postgres's pg_ tables.
  isSystemTable: (name: string) => boolean,
};

type Token = {
  type: string,
  from: number,
  to: number,
  text: string,
  parts: string[],
  depth: number,
};

type TableRef = {
  database: string | null,
  table: string,
  alias: string | null,
  columns: Set<string> | null,
};

const CHECKED_STATEMENTS = new Set(["select", "with", "update", "delete", "insert", "replace"]);
const TABLE_INTRO = new Set(["from", "join", "update", "into", "straight_join"]);
const IDENTIFIER_TYPES = new Set(["Identifier", "QuotedIdentifier"]);

function unquote(text: string): string {
  const quoted = /^([`"'[])(.*)([`"'\]])$/s.exec(text);
  return quoted ? quoted[2] : text;
}

function tokenize(statement: SyntaxNode, state: EditorState): Token[] {
  const tokens: Token[] = [];
  let depth = 0;

  function visit(node: SyntaxNode) {
    if (/Comment/.test(node.name)) return;

    if (node.name === "CompositeIdentifier") {
      const parts: string[] = [];
      for (let child = node.firstChild; child; child = child.nextSibling) {
        if (IDENTIFIER_TYPES.has(child.name)) parts.push(unquote(state.sliceDoc(child.from, child.to)));
      }
      tokens.push({ type: node.name, from: node.from, to: node.to, text: state.sliceDoc(node.from, node.to), parts, depth });
      return;
    }

    if (!node.firstChild) {
      const text = state.sliceDoc(node.from, node.to);
      if (node.name === ")") depth = Math.max(0, depth - 1);
      tokens.push({ type: node.name, from: node.from, to: node.to, text, parts: [unquote(text)], depth });
      if (node.name === "(") depth++;
      return;
    }

    for (let child: SyntaxNode | null = node.firstChild; child; child = child.nextSibling) visit(child);
  }

  visit(statement);
  return tokens;
}

function keyword(token: Token | undefined): string | null {
  return token?.type === "Keyword" ? token.text.toLowerCase() : null;
}

function isName(token: Token | undefined): boolean {
  return token !== undefined && (IDENTIFIER_TYPES.has(token.type) || token.type === "CompositeIdentifier");
}

// The type after a Postgres `::` cast, which isn't a column.
function isCastTarget(tokens: Token[], index: number): boolean {
  const previous = tokens[index - 1];
  if (previous?.text === "::") return true;
  return previous?.text === ":" && tokens[index - 2]?.text === ":";
}

function closingParen(tokens: Token[], open: number): number {
  const depth = tokens[open].depth;
  let end = open + 1;
  while (end < tokens.length && !(tokens[end].type === ")" && tokens[end].depth === depth)) end++;
  return end;
}

function lintStatement(tokens: Token[], catalog: SchemaCatalog, diagnostics: Diagnostic[]) {
  const first = keyword(tokens[0]);
  if (!first || !CHECKED_STATEMENTS.has(first)) return;

  const refs: TableRef[] = [];
  const opaque = new Set<string>();
  const aliases = new Set<string>();
  const consumed = new Set<number>();
  let unresolved = false;

  for (let index = 0; index < tokens.length; index++) {
    const token = tokens[index];
    if (IDENTIFIER_TYPES.has(token.type) && keyword(tokens[index + 1]) === "as" && tokens[index + 2]?.type === "(") {
      opaque.add(token.parts[0].toLowerCase());
      consumed.add(index);
    }
    if (keyword(token) === "conflict") opaque.add("excluded");
  }

  // A derived table or a function such as generate_series(...), with an optional alias.
  function readOpaqueSource(open: number): number {
    let next = closingParen(tokens, open) + 1;
    if (keyword(tokens[next]) === "as") next++;
    if (IDENTIFIER_TYPES.has(tokens[next]?.type ?? "")) {
      opaque.add(tokens[next].parts[0].toLowerCase());
      consumed.add(next);
      return next + 1;
    }
    return next;
  }

  function lookUp(ref: TableRef): { tables: Map<string, Set<string> | null> | null, complete: boolean } {
    const searched = ref.database !== null ? [ref.database] : catalog.searchPath;
    let complete = true;
    for (const name of searched) {
      const tables = catalog.tables.get(name);
      if (!tables) {
        complete = false;
        continue;
      }
      if (tables.has(ref.table)) return { tables, complete };
    }
    return { tables: null, complete };
  }

  function readTableRef(index: number): number {
    const token = tokens[index];

    if (token?.type === "(") return readOpaqueSource(index);

    if (!isName(token)) return index + 1;
    consumed.add(index);

    if (tokens[index + 1]?.type === "(") {
      unresolved = true;
      return readOpaqueSource(index + 1);
    }

    const parts = token.parts.map(part => part.toLowerCase());
    const ref: TableRef = {
      database: parts.length > 1 ? parts[parts.length - 2] : null,
      table: parts[parts.length - 1],
      alias: null,
      columns: null,
    };

    if (ref.database === null && (opaque.has(ref.table) || catalog.isSystemTable(ref.table))) {
      unresolved = true;
    } else if (ref.database !== null && !catalog.databases.has(ref.database)) {
      diagnostics.push({
        from: token.from,
        to: token.to,
        severity: "error",
        message: `Unknown ${catalog.namespace} '${token.parts[token.parts.length - 2]}'`,
      });
      unresolved = true;
    } else {
      const found = lookUp(ref);
      if (found.tables) {
        ref.columns = found.tables.get(ref.table) ?? null;
        if (ref.columns === null) unresolved = true;
      } else if (!found.complete) {
        unresolved = true;
      } else {
        diagnostics.push({
          from: token.from,
          to: token.to,
          severity: "error",
          message: `Table '${token.parts[token.parts.length - 1]}' doesn't exist in ${ref.database ?? catalog.database}`,
        });
        unresolved = true;
      }
    }

    let next = index + 1;
    if (keyword(tokens[next]) === "as") next++;
    if (IDENTIFIER_TYPES.has(tokens[next]?.type ?? "") && tokens[next].depth === token.depth) {
      ref.alias = tokens[next].parts[0].toLowerCase();
      consumed.add(next);
      next++;
    }

    refs.push(ref);
    return next;
  }

  for (let index = 0; index < tokens.length; index++) {
    const word = keyword(tokens[index]);
    if (!word || !TABLE_INTRO.has(word)) continue;
    if (word === "update" && keyword(tokens[index - 1]) === "key") continue;

    let cursor = readTableRef(index + 1);
    if (word === "from" || word === "update") {
      while (tokens[cursor]?.type === "Punctuation" && tokens[cursor].text === ",") {
        cursor = readTableRef(cursor + 1);
      }
    }
    index = cursor - 1;
  }

  for (let index = 0; index < tokens.length; index++) {
    const token = tokens[index];
    const previous = tokens[index - 1];
    if (!IDENTIFIER_TYPES.has(token.type) || consumed.has(index)) continue;

    const afterAs = keyword(previous) === "as";
    const implicitAlias = previous !== undefined
      && (isName(previous) || [")", "String", "Number"].includes(previous.type))
      && !consumed.has(index - 1);
    if (afterAs || implicitAlias) aliases.add(token.parts[0].toLowerCase());
  }

  if (refs.length === 0) return;

  const byName = new Map<string, TableRef>();
  for (const ref of refs) {
    byName.set(ref.alias ?? ref.table, ref);
    if (!ref.alias) byName.set(ref.table, ref);
  }

  for (let index = 0; index < tokens.length; index++) {
    const token = tokens[index];
    if (!isName(token) || consumed.has(index)) continue;

    const next = tokens[index + 1];
    const previous = keyword(tokens[index - 1]);
    if (next?.type === "(" || isCastTarget(tokens, index)) continue;
    if (previous === "as" || previous === "collate" || previous === "charset" || previous === "using" && next?.type !== "(") continue;
    if (previous === "set" && keyword(tokens[index - 2]) === "character") continue;

    const parts = token.parts.map(part => part.toLowerCase());

    if (parts.length === 1) {
      const name = parts[0];
      if (aliases.has(name) || unresolved) continue;
      if (refs.some(ref => ref.columns?.has(name))) continue;
      diagnostics.push({ from: token.from, to: token.to, severity: "error", message: `Unknown column '${token.parts[0]}'` });
      continue;
    }

    const qualifier = parts[parts.length - 2];
    const column = parts[parts.length - 1];
    const ref = byName.get(qualifier);

    if (!ref) {
      if (opaque.has(qualifier) || unresolved) continue;
      diagnostics.push({
        from: token.from,
        to: token.to,
        severity: "error",
        message: `Unknown table or alias '${token.parts[token.parts.length - 2]}'`,
      });
      continue;
    }

    if (ref.columns && !ref.columns.has(column)) {
      diagnostics.push({
        from: token.from,
        to: token.to,
        severity: "error",
        message: `Unknown column '${token.parts[token.parts.length - 1]}' in ${ref.table}`,
      });
    }
  }
}

export function lintSql(state: EditorState, catalog: SchemaCatalog): Diagnostic[] {
  const diagnostics: Diagnostic[] = [];
  const tree = syntaxTree(state);

  for (let statement = tree.topNode.firstChild; statement; statement = statement.nextSibling) {
    if (statement.name !== "Statement") continue;
    lintStatement(tokenize(statement, state), catalog, diagnostics);
  }

  return diagnostics;
}

export function schemaLinter(getCatalog: () => SchemaCatalog | null) {
  return linter(view => {
    const catalog = getCatalog();
    return catalog ? lintSql(view.state, catalog) : [];
  }, { delay: 400 });
}
