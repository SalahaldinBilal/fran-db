use super::splitter;
use super::sqlgen::Dialect;
use crate::db::classify::{ClassifiedStatement, Rules, Syntax, classify_statement};

const DENIED_FUNCTIONS: [&str; 12] = [
    "GET_LOCK",
    "RELEASE_LOCK",
    "RELEASE_ALL_LOCKS",
    "LOAD_FILE",
    "MASTER_POS_WAIT",
    "SOURCE_POS_WAIT",
    "WAIT_FOR_EXECUTED_GTID_SET",
    "WAIT_UNTIL_SQL_THREAD_AFTER_GTIDS",
    "NEXTVAL",
    "SETVAL",
    "SYS_EXEC",
    "SYS_EVAL",
];

fn rules(dialect: Dialect) -> Rules<'static> {
    Rules {
        syntax: Syntax {
            hash_comments: true,
            dash_comment_needs_space: true,
            executable_comments: true,
            backslash_escapes: !dialect.no_backslash_escapes,
            double_quote_strings: true,
            refuse_backslash_in_double_quotes: true,
            backtick_identifiers: true,
            ..Syntax::default()
        },
        queries: &["SELECT", "WITH", "VALUES", "TABLE"],
        explains: &["EXPLAIN", "DESCRIBE", "DESC"],
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
            reads("select 1; show create table t; describe t; insert into t values (1); select * from t lock in share mode"),
            vec![true, true, true, false, false]
        );
        assert_eq!(reads("select get_lock('a', 1)"), vec![false]);
        assert_eq!(reads("select * from t into outfile '/tmp/x'"), vec![false]);
        assert_eq!(reads("select next value for s"), vec![false]);
        assert_eq!(reads("desc analyze select 1"), vec![false]);
        assert_eq!(reads("do sleep(1)"), vec![false]);
    }

    #[test]
    fn refuses_executable_comments() {
        assert_eq!(reads("select 1 /*!50000 , (delete from t) */"), vec![false]);
        assert_eq!(reads("select /*+ MAX_EXECUTION_TIME(10) */ 1"), vec![true]);
    }

    #[test]
    fn follows_the_backslash_mode() {
        let sql = r"select 'a\'; delete from t; -- '";
        assert_eq!(reads(sql), vec![true]);
        let strict = classify(sql, Dialect { no_backslash_escapes: true });
        assert!(strict.iter().any(|statement| !statement.access.is_read()));
        assert_eq!(reads(r#"select "a\"b""#), vec![false]);
    }
}
