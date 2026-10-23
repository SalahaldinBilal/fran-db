use mysql_async::{
    Column, Value,
    consts::{ColumnFlags, ColumnType},
};

use crate::db::types::{ColumnChoices, ColumnMeta, ValueCategory};

const BINARY_CHARSET: u16 = 63;

pub fn column_meta(column: &Column) -> ColumnMeta {
    let column_type = column.column_type();
    let flags = column.flags();
    let binary = column.character_set() == BINARY_CHARSET;
    let (category, base_name) = classify(column_type, flags, binary);

    let type_name = if flags.contains(ColumnFlags::UNSIGNED_FLAG)
        && matches!(category, ValueCategory::Integer | ValueCategory::Decimal | ValueCategory::Float)
    {
        format!("{base_name} UNSIGNED")
    } else {
        base_name.to_string()
    };

    ColumnMeta {
        name: column.name_str().to_string(),
        type_name,
        category,
        nullable: !flags.contains(ColumnFlags::NOT_NULL_FLAG),
        primary_key: flags.contains(ColumnFlags::PRI_KEY_FLAG),
        database: non_empty(&column.schema_str()),
        table: non_empty(&column.org_table_str()),
        source_name: non_empty(&column.org_name_str()),
        has_choices: matches!(base_name, "ENUM" | "SET"),
    }
}

fn non_empty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_string())
}

fn classify(column_type: ColumnType, flags: ColumnFlags, binary: bool) -> (ValueCategory, &'static str) {
    use ColumnType::*;

    if flags.contains(ColumnFlags::ENUM_FLAG) {
        return (ValueCategory::Text, "ENUM");
    }
    if flags.contains(ColumnFlags::SET_FLAG) {
        return (ValueCategory::Text, "SET");
    }

    match column_type {
        MYSQL_TYPE_TINY => (ValueCategory::Integer, "TINYINT"),
        MYSQL_TYPE_SHORT => (ValueCategory::Integer, "SMALLINT"),
        MYSQL_TYPE_INT24 => (ValueCategory::Integer, "MEDIUMINT"),
        MYSQL_TYPE_LONG => (ValueCategory::Integer, "INT"),
        MYSQL_TYPE_LONGLONG => (ValueCategory::Integer, "BIGINT"),
        MYSQL_TYPE_YEAR => (ValueCategory::Integer, "YEAR"),
        MYSQL_TYPE_DECIMAL | MYSQL_TYPE_NEWDECIMAL => (ValueCategory::Decimal, "DECIMAL"),
        MYSQL_TYPE_FLOAT => (ValueCategory::Float, "FLOAT"),
        MYSQL_TYPE_DOUBLE => (ValueCategory::Float, "DOUBLE"),
        MYSQL_TYPE_BIT => (ValueCategory::Bit, "BIT"),
        MYSQL_TYPE_DATE | MYSQL_TYPE_NEWDATE => (ValueCategory::Temporal, "DATE"),
        MYSQL_TYPE_TIME | MYSQL_TYPE_TIME2 => (ValueCategory::Temporal, "TIME"),
        MYSQL_TYPE_DATETIME | MYSQL_TYPE_DATETIME2 => (ValueCategory::Temporal, "DATETIME"),
        MYSQL_TYPE_TIMESTAMP | MYSQL_TYPE_TIMESTAMP2 => (ValueCategory::Temporal, "TIMESTAMP"),
        MYSQL_TYPE_JSON => (ValueCategory::Json, "JSON"),
        MYSQL_TYPE_GEOMETRY => (ValueCategory::Spatial, "GEOMETRY"),
        MYSQL_TYPE_ENUM => (ValueCategory::Text, "ENUM"),
        MYSQL_TYPE_SET => (ValueCategory::Text, "SET"),
        MYSQL_TYPE_TINY_BLOB | MYSQL_TYPE_MEDIUM_BLOB | MYSQL_TYPE_LONG_BLOB | MYSQL_TYPE_BLOB => {
            if binary {
                (ValueCategory::Binary, "BLOB")
            } else {
                (ValueCategory::Text, "TEXT")
            }
        }
        MYSQL_TYPE_VARCHAR | MYSQL_TYPE_VAR_STRING => {
            if binary {
                (ValueCategory::Binary, "VARBINARY")
            } else {
                (ValueCategory::Text, "VARCHAR")
            }
        }
        MYSQL_TYPE_STRING => {
            if binary {
                (ValueCategory::Binary, "BINARY")
            } else {
                (ValueCategory::Text, "CHAR")
            }
        }
        MYSQL_TYPE_NULL => (ValueCategory::Other, "NULL"),
        _ => (ValueCategory::Other, "UNKNOWN"),
    }
}

pub fn format_value(value: Option<&Value>, category: ValueCategory) -> Option<String> {
    let value = value?;

    match value {
        Value::NULL => None,
        Value::Bytes(bytes) => Some(match category {
            ValueCategory::Binary | ValueCategory::Spatial => format!("0x{}", hex::encode_upper(bytes)),
            ValueCategory::Bit => bit_to_string(bytes),
            _ => String::from_utf8_lossy(bytes).into_owned(),
        }),
        Value::Int(number) => Some(number.to_string()),
        Value::UInt(number) => Some(number.to_string()),
        Value::Float(number) => Some(number.to_string()),
        Value::Double(number) => Some(number.to_string()),
        Value::Date(year, month, day, hour, minute, second, micros) => Some(
            if *hour == 0 && *minute == 0 && *second == 0 && *micros == 0 {
                format!("{year:04}-{month:02}-{day:02}")
            } else if *micros == 0 {
                format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}")
            } else {
                format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}.{micros:06}")
            },
        ),
        Value::Time(negative, days, hours, minutes, seconds, micros) => {
            let sign = if *negative { "-" } else { "" };
            let total_hours = u64::from(*days) * 24 + u64::from(*hours);
            Some(if *micros == 0 {
                format!("{sign}{total_hours:02}:{minutes:02}:{seconds:02}")
            } else {
                format!("{sign}{total_hours:02}:{minutes:02}:{seconds:02}.{micros:06}")
            })
        }
    }
}

fn bit_to_string(bytes: &[u8]) -> String {
    let value = bytes
        .iter()
        .fold(0u128, |accumulator, byte| (accumulator << 8) | u128::from(*byte));
    value.to_string()
}

/// Reads the value list out of a COLUMN_TYPE such as `enum('a','it''s')` or `set('x','y')`.
pub fn parse_choices(column_type: &str) -> Option<ColumnChoices> {
    let lower = column_type.to_ascii_lowercase();
    let (multiple, start) = if lower.starts_with("enum(") {
        (false, 5)
    } else if lower.starts_with("set(") {
        (true, 4)
    } else {
        return None;
    };
    let body = column_type.get(start..column_type.rfind(')')?)?;

    let mut values = Vec::new();
    let mut chars = body.chars().peekable();
    loop {
        match chars.next() {
            None => break,
            Some(',') | Some(' ') => continue,
            Some('\'') => {}
            Some(_) => return None,
        }

        let mut value = String::new();
        loop {
            match chars.next()? {
                '\'' if chars.peek() == Some(&'\'') => {
                    chars.next();
                    value.push('\'');
                }
                '\'' => break,
                other => value.push(other),
            }
        }
        values.push(value);
    }

    Some(ColumnChoices { multiple, values })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_enum_and_set_values() {
        assert_eq!(
            parse_choices("enum('small','it''s, big','')"),
            Some(ColumnChoices {
                multiple: false,
                values: vec!["small".into(), "it's, big".into(), String::new()],
            })
        );
        assert_eq!(
            parse_choices("set('a','b')"),
            Some(ColumnChoices { multiple: true, values: vec!["a".into(), "b".into()] })
        );
        assert_eq!(parse_choices("varchar(20)"), None);
    }
}
