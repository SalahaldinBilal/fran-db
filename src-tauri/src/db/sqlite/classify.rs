use super::splitter;
use crate::db::classify::{ClassifiedStatement, Rules, StatementAccess, Syntax, Token, classify_statement};

const DENIED_FUNCTIONS: [&str; 5] = ["LOAD_EXTENSION", "WRITEFILE", "READFILE", "EDIT", "FTS3_TOKENIZER"];

/// Pragmas that only report, with or without an argument naming what to report on.
const QUERY_PRAGMAS: [&str; 10] = [
    "TABLE_INFO",
    "TABLE_XINFO",
    "TABLE_LIST",
    "INDEX_LIST",
    "INDEX_INFO",
    "INDEX_XINFO",
    "FOREIGN_KEY_LIST",
    "FOREIGN_KEY_CHECK",
    "INTEGRITY_CHECK",
    "QUICK_CHECK",
];

/// Pragmas that report a setting when called bare and change it when given a value.
const VALUE_PRAGMAS: [&str; 20] = [
    "DATABASE_LIST",
    "COLLATION_LIST",
    "FUNCTION_LIST",
    "MODULE_LIST",
    "PRAGMA_LIST",
    "COMPILE_OPTIONS",
    "USER_VERSION",
    "SCHEMA_VERSION",
    "APPLICATION_ID",
    "DATA_VERSION",
    "PAGE_COUNT",
    "PAGE_SIZE",
    "FREELIST_COUNT",
    "ENCODING",
    "JOURNAL_MODE",
    "FOREIGN_KEYS",
    "AUTO_VACUUM",
    "CACHE_SIZE",
    "SYNCHRONOUS",
    "BUSY_TIMEOUT",
];

fn pragma_access(tokens: &[Token]) -> Option<StatementAccess> {
    let (first, rest) = tokens.split_first()?;
    if *first != Token::Word("PRAGMA".to_string()) {
        return None;
    }

    let rest = match rest {
        [_, Token::Symbol('.'), rest @ ..] => rest,
        rest => rest,
    };
    let Some((Token::Word(name) | Token::Identifier(name), arguments)) = rest.split_first() else {
        return Some(StatementAccess::change("The pragma has no name"));
    };

    if arguments.contains(&Token::Symbol('=')) {
        return Some(StatementAccess::change(format!("PRAGMA {} = ... changes a setting", name.to_lowercase())));
    }
    if QUERY_PRAGMAS.contains(&name.as_str()) || (arguments.is_empty() && VALUE_PRAGMAS.contains(&name.as_str())) {
        return Some(StatementAccess::Read);
    }
    Some(StatementAccess::change(format!("PRAGMA {} isn't one of the pragmas that only report", name.to_lowercase())))
}

const RULES: Rules = Rules {
    syntax: Syntax {
        hash_comments: false,
        dash_comment_needs_space: false,
        nested_comments: false,
        executable_comments: false,
        dollar_quotes: false,
        escape_strings: false,
        backslash_escapes: false,
        double_quote_strings: false,
        refuse_backslash_in_double_quotes: false,
        backtick_identifiers: true,
        bracket_identifiers: true,
    },
    queries: &["SELECT", "WITH", "VALUES"],
    explains: &["EXPLAIN"],
    shows: &[],
    denied_functions: &DENIED_FUNCTIONS,
    inspect: Some(pragma_access),
};

pub fn classify(sql: &str) -> Vec<ClassifiedStatement> {
    splitter::split(sql)
        .into_iter()
        .map(|statement| ClassifiedStatement {
            access: classify_statement(&statement.text, &RULES),
            text: statement.text,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reads(sql: &str) -> Vec<bool> {
        classify(sql).iter().map(|statement| statement.access.is_read()).collect()
    }

    #[test]
    fn tells_reads_from_changes() {
        assert_eq!(
            reads("select [delete] from t; explain query plan delete from t; attach 'x.db' as x; replace into t values (1)"),
            vec![true, true, false, false]
        );
        assert_eq!(reads("with gone as (select 1) delete from t"), vec![false]);
        assert_eq!(reads("select load_extension('evil')"), vec![false]);
    }

    #[test]
    fn allows_only_reporting_pragmas() {
        assert_eq!(
            reads(
                "pragma table_info('t'); pragma main.index_list(\"t\"); pragma user_version; \
                 pragma user_version = 3; pragma journal_mode(wal); pragma writable_schema"
            ),
            vec![true, true, true, false, false, false]
        );
    }
}
