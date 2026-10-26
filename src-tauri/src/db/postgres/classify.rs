use super::splitter;
use super::sqlgen::Dialect;
use crate::db::classify::{ClassifiedStatement, Rules, Syntax, classify_statement};

/// Functions that write, lock, signal other sessions, touch server files, or run SQL from a string.
const DENIED_FUNCTIONS: [&str; 62] = [
    "SET_CONFIG",
    "NEXTVAL",
    "SETVAL",
    "TXID_CURRENT",
    "PG_CURRENT_XACT_ID",
    "PG_TERMINATE_BACKEND",
    "PG_CANCEL_BACKEND",
    "PG_RELOAD_CONF",
    "PG_ROTATE_LOGFILE",
    "PG_PROMOTE",
    "PG_SWITCH_WAL",
    "PG_CREATE_RESTORE_POINT",
    "PG_LOG_BACKEND_MEMORY_CONTEXTS",
    "PG_ADVISORY_LOCK",
    "PG_ADVISORY_LOCK_SHARED",
    "PG_TRY_ADVISORY_LOCK",
    "PG_TRY_ADVISORY_LOCK_SHARED",
    "PG_ADVISORY_XACT_LOCK",
    "PG_ADVISORY_XACT_LOCK_SHARED",
    "PG_TRY_ADVISORY_XACT_LOCK",
    "PG_TRY_ADVISORY_XACT_LOCK_SHARED",
    "PG_ADVISORY_UNLOCK",
    "PG_ADVISORY_UNLOCK_SHARED",
    "PG_ADVISORY_UNLOCK_ALL",
    "LO_IMPORT",
    "LO_EXPORT",
    "LO_UNLINK",
    "LO_CREATE",
    "LO_CREAT",
    "LO_FROM_BYTEA",
    "LO_PUT",
    "LO_TRUNCATE",
    "LO_OPEN",
    "LOWRITE",
    "DBLINK",
    "DBLINK_EXEC",
    "DBLINK_CONNECT",
    "DBLINK_CONNECT_U",
    "DBLINK_SEND_QUERY",
    "DBLINK_OPEN",
    "PG_READ_FILE",
    "PG_READ_BINARY_FILE",
    "PG_LS_DIR",
    "PG_STAT_FILE",
    "PG_FILE_WRITE",
    "PG_FILE_RENAME",
    "PG_FILE_UNLINK",
    "PG_NOTIFY",
    "PG_LOGICAL_EMIT_MESSAGE",
    "PG_CREATE_LOGICAL_REPLICATION_SLOT",
    "PG_CREATE_PHYSICAL_REPLICATION_SLOT",
    "PG_DROP_REPLICATION_SLOT",
    "PG_REPLICATION_ORIGIN_CREATE",
    "PG_REPLICATION_ORIGIN_DROP",
    "PG_STAT_RESET",
    "PG_STAT_RESET_SHARED",
    "PG_STAT_STATEMENTS_RESET",
    "QUERY_TO_XML",
    "QUERY_TO_XMLSCHEMA",
    "QUERY_TO_XML_AND_XMLSCHEMA",
    "CURSOR_TO_XML",
    "CURSOR_TO_XMLSCHEMA",
];

fn rules(dialect: Dialect) -> Rules<'static> {
    Rules {
        syntax: Syntax {
            nested_comments: true,
            dollar_quotes: true,
            escape_strings: true,
            backslash_escapes: !dialect.standard_conforming_strings,
            ..Syntax::default()
        },
        queries: &["SELECT", "WITH", "VALUES", "TABLE"],
        explains: &["EXPLAIN"],
        shows: &["SHOW"],
        denied_functions: &DENIED_FUNCTIONS,
        inspect: None,
    }
}

pub fn classify(sql: &str, dialect: Dialect) -> Vec<ClassifiedStatement> {
    let rules = rules(dialect);
    splitter::split(sql)
        .into_iter()
        .map(|statement| ClassifiedStatement {
            access: classify_statement(&statement.text, &rules),
            text: statement.text,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reads(sql: &str) -> Vec<bool> {
        classify(sql, Dialect::default()).iter().map(|statement| statement.access.is_read()).collect()
    }

    #[test]
    fn tells_reads_from_changes() {
        assert_eq!(
            reads("select 1; table users; show work_mem; explain update t set a = 1; begin; set search_path = x"),
            vec![true, true, true, true, false, false]
        );
        assert_eq!(reads("select pg_catalog.set_config('a', 'b', false)"), vec![false]);
        assert_eq!(reads("select nextval('s')"), vec![false]);
        assert_eq!(reads("select query_to_xml('delete from t', true, false, '')"), vec![false]);
        assert_eq!(reads("select * from t for no key update"), vec![false]);
        assert_eq!(reads("select * from t for key share"), vec![false]);
    }

    #[test]
    fn keeps_dollar_quoted_text_out() {
        assert_eq!(reads("select $q$ delete from t $q$"), vec![true]);
        assert_eq!(reads("select /* nested /* delete */ */ 1"), vec![true]);
    }
}
