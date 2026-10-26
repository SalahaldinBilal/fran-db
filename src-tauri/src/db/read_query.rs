use super::types::{FilterOp, FilterValue, ReadQuery, RowFilter, RowOrder};
use crate::error::{AppError, AppResult};

/// The quoting rules of one database, supplied by its driver.
pub trait SqlWriter {
    fn identifier(&self, name: &str) -> String;
    fn table(&self, database: &str, table: &str) -> String;
    fn string(&self, value: &str) -> String;
    fn boolean(&self, value: bool) -> String;
}

pub fn read_sql(writer: &dyn SqlWriter, database: &str, query: &ReadQuery) -> AppResult<String> {
    match query {
        ReadQuery::Rows { table, columns, filters, order, limit, offset } => {
            let list = if columns.is_empty() {
                "*".to_string()
            } else {
                columns.iter().map(|column| writer.identifier(column)).collect::<Vec<_>>().join(", ")
            };
            let mut sql = format!("SELECT {list} FROM {}", writer.table(database, table));
            sql.push_str(&where_clause(writer, filters)?);
            sql.push_str(&order_clause(writer, order));
            sql.push_str(&format!(" LIMIT {limit}"));
            if *offset > 0 {
                sql.push_str(&format!(" OFFSET {offset}"));
            }
            Ok(sql)
        }
        ReadQuery::Count { table, filters } => Ok(format!(
            "SELECT COUNT(*) AS {} FROM {}{}",
            writer.identifier("count"),
            writer.table(database, table),
            where_clause(writer, filters)?
        )),
        ReadQuery::ColumnValues { table, column, filters, limit } => {
            let column = writer.identifier(column);
            let count = writer.identifier("count");
            Ok(format!(
                "SELECT {column}, COUNT(*) AS {count} FROM {}{} GROUP BY {column} ORDER BY {count} DESC, {column} LIMIT {limit}",
                writer.table(database, table),
                where_clause(writer, filters)?
            ))
        }
    }
}

fn order_clause(writer: &dyn SqlWriter, order: &[RowOrder]) -> String {
    if order.is_empty() {
        return String::new();
    }
    let terms: Vec<String> = order
        .iter()
        .map(|term| format!("{} {}", writer.identifier(&term.column), if term.descending { "DESC" } else { "ASC" }))
        .collect();
    format!(" ORDER BY {}", terms.join(", "))
}

fn where_clause(writer: &dyn SqlWriter, filters: &[RowFilter]) -> AppResult<String> {
    if filters.is_empty() {
        return Ok(String::new());
    }
    let conditions = filters
        .iter()
        .map(|filter| condition(writer, filter))
        .collect::<AppResult<Vec<_>>>()?;
    Ok(format!(" WHERE {}", conditions.join(" AND ")))
}

fn condition(writer: &dyn SqlWriter, filter: &RowFilter) -> AppResult<String> {
    let column = writer.identifier(&filter.column);
    let values = &filter.values;
    let expect = |count: usize, what: &str| -> AppResult<()> {
        if values.len() == count {
            Ok(())
        } else {
            Err(AppError::general(format!("The filter on {} takes {what}", filter.column)))
        }
    };

    let comparison = |operator: &str| -> AppResult<String> {
        expect(1, "one value")?;
        if values[0] == FilterValue::Null {
            return Err(AppError::general(format!(
                "The filter on {} compares with null, which never matches. Use isNull or isNotNull.",
                filter.column
            )));
        }
        Ok(format!("{column} {operator} {}", literal(writer, &values[0])?))
    };

    let only_null = matches!(values.as_slice(), [FilterValue::Null]);
    match filter.op {
        FilterOp::Eq if only_null => Ok(format!("{column} IS NULL")),
        FilterOp::Ne if only_null => Ok(format!("{column} IS NOT NULL")),
        FilterOp::Eq => comparison("="),
        FilterOp::Ne => comparison("<>"),
        FilterOp::Lt => comparison("<"),
        FilterOp::Lte => comparison("<="),
        FilterOp::Gt => comparison(">"),
        FilterOp::Gte => comparison(">="),
        FilterOp::Like => comparison("LIKE"),
        FilterOp::NotLike => comparison("NOT LIKE"),
        FilterOp::IsNull => expect(0, "no value").map(|()| format!("{column} IS NULL")),
        FilterOp::IsNotNull => expect(0, "no value").map(|()| format!("{column} IS NOT NULL")),
        FilterOp::Between => {
            expect(2, "two values")?;
            Ok(format!("{column} BETWEEN {} AND {}", literal(writer, &values[0])?, literal(writer, &values[1])?))
        }
        FilterOp::In | FilterOp::NotIn => {
            if values.is_empty() {
                return Err(AppError::general(format!("The filter on {} needs at least one value", filter.column)));
            }
            let list = values.iter().map(|value| literal(writer, value)).collect::<AppResult<Vec<_>>>()?;
            let operator = if filter.op == FilterOp::In { "IN" } else { "NOT IN" };
            Ok(format!("{column} {operator} ({})", list.join(", ")))
        }
    }
}

fn literal(writer: &dyn SqlWriter, value: &FilterValue) -> AppResult<String> {
    match value {
        FilterValue::Null => Ok("NULL".to_string()),
        FilterValue::Bool(value) => Ok(writer.boolean(*value)),
        FilterValue::Number(value) if is_number(value) => Ok(value.clone()),
        FilterValue::Number(value) => Err(AppError::general(format!("{value} isn't a number"))),
        FilterValue::Text(value) => Ok(writer.string(value)),
    }
}

fn is_number(value: &str) -> bool {
    let unsigned = value.strip_prefix('-').unwrap_or(value);
    let (mantissa, exponent) = match unsigned.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, Some(exponent)),
        None => (unsigned, None),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = |part: &str| part.chars().all(|character| character.is_ascii_digit());
    let exponent_ok = exponent.is_none_or(|exponent| {
        let exponent = exponent.strip_prefix(['-', '+']).unwrap_or(exponent);
        !exponent.is_empty() && digits(exponent)
    });
    !whole.is_empty() && digits(whole) && digits(fraction) && exponent_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Ansi;

    impl SqlWriter for Ansi {
        fn identifier(&self, name: &str) -> String {
            format!("\"{}\"", name.replace('"', "\"\""))
        }
        fn table(&self, database: &str, table: &str) -> String {
            format!("{}.{}", self.identifier(database), self.identifier(table))
        }
        fn string(&self, value: &str) -> String {
            format!("'{}'", value.replace('\'', "''"))
        }
        fn boolean(&self, value: bool) -> String {
            if value { "TRUE" } else { "FALSE" }.to_string()
        }
    }

    fn filter(column: &str, op: FilterOp, values: Vec<FilterValue>) -> RowFilter {
        RowFilter { column: column.to_string(), op, values }
    }

    #[test]
    fn writes_row_reads() {
        let query = ReadQuery::Rows {
            table: "users".into(),
            columns: vec!["id".into(), "na\"me".into()],
            filters: vec![
                filter("age", FilterOp::Between, vec![FilterValue::Number("18".into()), FilterValue::Number("6.5e1".into())]),
                filter("name", FilterOp::Like, vec![FilterValue::Text("O'%".into())]),
                filter("team", FilterOp::In, vec![FilterValue::Number("1".into()), FilterValue::Null]),
                filter("deleted", FilterOp::Eq, vec![FilterValue::Null]),
                filter("active", FilterOp::Ne, vec![FilterValue::Bool(false)]),
            ],
            order: vec![RowOrder { column: "id".into(), descending: true }],
            limit: 50,
            offset: 100,
        };
        assert_eq!(
            read_sql(&Ansi, "app", &query).unwrap(),
            "SELECT \"id\", \"na\"\"me\" FROM \"app\".\"users\" WHERE \"age\" BETWEEN 18 AND 6.5e1 AND \"name\" LIKE 'O''%' \
             AND \"team\" IN (1, NULL) AND \"deleted\" IS NULL AND \"active\" <> FALSE ORDER BY \"id\" DESC LIMIT 50 OFFSET 100"
        );
    }

    #[test]
    fn writes_counts_and_value_lists() {
        let count = ReadQuery::Count { table: "t".into(), filters: vec![filter("a", FilterOp::IsNotNull, vec![])] };
        assert_eq!(
            read_sql(&Ansi, "s", &count).unwrap(),
            "SELECT COUNT(*) AS \"count\" FROM \"s\".\"t\" WHERE \"a\" IS NOT NULL"
        );
        let values = ReadQuery::ColumnValues { table: "t".into(), column: "kind".into(), filters: vec![], limit: 20 };
        assert_eq!(
            read_sql(&Ansi, "s", &values).unwrap(),
            "SELECT \"kind\", COUNT(*) AS \"count\" FROM \"s\".\"t\" GROUP BY \"kind\" ORDER BY \"count\" DESC, \"kind\" LIMIT 20"
        );
    }

    #[test]
    fn refuses_bad_filters() {
        let bad = |op, values| {
            let query = ReadQuery::Count { table: "t".into(), filters: vec![filter("a", op, values)] };
            read_sql(&Ansi, "s", &query).is_err()
        };
        assert!(bad(FilterOp::Eq, vec![FilterValue::Number("1; drop table t".into())]));
        assert!(bad(FilterOp::Gt, vec![FilterValue::Null]));
        assert!(bad(FilterOp::Between, vec![FilterValue::Number("1".into())]));
        assert!(bad(FilterOp::In, vec![]));
        assert!(bad(FilterOp::IsNull, vec![FilterValue::Bool(true)]));
    }
}
