use std::{collections::HashMap, sync::Mutex};

use super::client::{PgConnection, PgError};
use super::metadata::{name, text};
use crate::db::types::ValueCategory;

#[derive(Debug, Clone)]
pub struct PgType {
    pub name: String,
    pub category: ValueCategory,
    pub enumerated: bool,
}

pub type TypeCache = Mutex<HashMap<u32, PgType>>;

const NUMERIC: u32 = 1700;
const BPCHAR: u32 = 1042;
const VARCHAR: u32 = 1043;
const BIT: u32 = 1560;
const VARBIT: u32 = 1562;

fn builtin(oid: u32) -> Option<(&'static str, ValueCategory)> {
    use ValueCategory::*;

    Some(match oid {
        16 => ("BOOLEAN", Boolean),
        17 => ("BYTEA", Binary),
        18 => ("\"CHAR\"", Text),
        19 => ("NAME", Text),
        20 => ("BIGINT", Integer),
        21 => ("SMALLINT", Integer),
        23 => ("INTEGER", Integer),
        24 => ("REGPROC", Text),
        25 => ("TEXT", Text),
        26 => ("OID", Integer),
        28 => ("XID", Text),
        114 => ("JSON", Json),
        142 => ("XML", Text),
        600 => ("POINT", Text),
        601 => ("LSEG", Text),
        602 => ("PATH", Text),
        603 => ("BOX", Text),
        604 => ("POLYGON", Text),
        628 => ("LINE", Text),
        650 => ("CIDR", Text),
        700 => ("REAL", Float),
        701 => ("DOUBLE PRECISION", Float),
        705 => ("UNKNOWN", Text),
        718 => ("CIRCLE", Text),
        774 => ("MACADDR8", Text),
        790 => ("MONEY", Text),
        829 => ("MACADDR", Text),
        869 => ("INET", Text),
        1033 => ("ACLITEM", Text),
        BPCHAR => ("CHAR", Text),
        VARCHAR => ("VARCHAR", Text),
        1082 => ("DATE", Temporal),
        1083 => ("TIME", Temporal),
        1114 => ("TIMESTAMP", Temporal),
        1184 => ("TIMESTAMPTZ", Temporal),
        1186 => ("INTERVAL", Text),
        1266 => ("TIMETZ", Temporal),
        BIT => ("BIT", Text),
        VARBIT => ("VARBIT", Text),
        NUMERIC => ("NUMERIC", Decimal),
        2205 => ("REGCLASS", Text),
        2206 => ("REGTYPE", Text),
        2249 => ("RECORD", Text),
        2278 => ("VOID", Other),
        2950 => ("UUID", Text),
        3220 => ("PG_LSN", Text),
        3614 => ("TSVECTOR", Text),
        3615 => ("TSQUERY", Text),
        3802 => ("JSONB", Json),
        3904 => ("INT4RANGE", Text),
        3906 => ("NUMRANGE", Text),
        3908 => ("TSRANGE", Text),
        3910 => ("TSTZRANGE", Text),
        3912 => ("DATERANGE", Text),
        3926 => ("INT8RANGE", Text),
        _ => return None,
    })
}

fn builtin_array_element(oid: u32) -> Option<u32> {
    Some(match oid {
        199 => 114,
        1000 => 16,
        1001 => 17,
        1003 => 19,
        1005 => 21,
        1007 => 23,
        1009 => 25,
        1014 => BPCHAR,
        1015 => VARCHAR,
        1016 => 20,
        1021 => 700,
        1022 => 701,
        1028 => 26,
        1115 => 1114,
        1182 => 1082,
        1183 => 1083,
        1185 => 1184,
        1231 => NUMERIC,
        2951 => 2950,
        3807 => 3802,
        _ => return None,
    })
}

fn known(oid: u32) -> Option<PgType> {
    if let Some((name, category)) = builtin(oid) {
        return Some(PgType {
            name: name.to_string(),
            category,
            enumerated: false,
        });
    }

    let (element, _) = builtin(builtin_array_element(oid)?)?;
    Some(PgType {
        name: format!("{element}[]"),
        category: ValueCategory::Text,
        enumerated: false,
    })
}

fn classify(type_name: &str, kind: &str, category: &str) -> (ValueCategory, bool) {
    if kind == "e" {
        return (ValueCategory::Text, true);
    }
    if matches!(type_name, "geometry" | "geography") {
        return (ValueCategory::Spatial, false);
    }
    let category = match category {
        "B" => ValueCategory::Boolean,
        "N" => ValueCategory::Decimal,
        _ => ValueCategory::Text,
    };
    (category, false)
}

/// Looks up every type in `oids`. Built-in types and earlier lookups come from memory.
pub async fn resolve(conn: &mut PgConnection, cache: &TypeCache, oids: &[u32]) -> Result<HashMap<u32, PgType>, PgError> {
    let mut found = HashMap::new();
    let mut missing = Vec::new();

    {
        let cached = cache.lock().map_err(|_| PgError::Transport("Type cache lock poisoned".to_string()))?;
        for &oid in oids {
            if found.contains_key(&oid) {
                continue;
            }
            if let Some(pg_type) = known(oid).or_else(|| cached.get(&oid).cloned()) {
                found.insert(oid, pg_type);
            } else if !missing.contains(&oid) {
                missing.push(oid);
            }
        }
    }

    if missing.is_empty() {
        return Ok(found);
    }

    let list = format!(
        "{{{}}}",
        missing.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
    );
    let rows = conn
        .query_text(
            "SELECT t.oid::text, t.typname, t.typtype::text, t.typcategory::text, e.typname \
             FROM pg_type t LEFT JOIN pg_type e ON e.oid = t.typelem AND t.typcategory = 'A' \
             WHERE t.oid = ANY($1::oid[])",
            &[Some(&list)],
        )
        .await?;

    let mut cached = cache.lock().map_err(|_| PgError::Transport("Type cache lock poisoned".to_string()))?;
    for row in &rows {
        let Some(oid) = text(row, 0).and_then(|oid| oid.parse::<u32>().ok()) else {
            continue;
        };
        let type_name = name(row, 1);
        let pg_type = match text(row, 4) {
            Some(element) => PgType {
                name: format!("{element}[]"),
                category: ValueCategory::Text,
                enumerated: false,
            },
            None => {
                let (category, enumerated) = classify(&type_name, &name(row, 2), &name(row, 3));
                PgType {
                    name: type_name,
                    category,
                    enumerated,
                }
            }
        };
        cached.insert(oid, pg_type.clone());
        found.insert(oid, pg_type);
    }

    Ok(found)
}

/// The type of a value whose OID isn't in `pg_type`, which happens when the type was created in an
/// uncommitted transaction on another connection.
pub fn unresolved(oid: u32) -> PgType {
    known(oid).unwrap_or_else(|| PgType {
        name: format!("type {oid}"),
        category: ValueCategory::Other,
        enumerated: false,
    })
}

/// Adds the length or precision from a column's type modifier, like `VARCHAR(40)` or `NUMERIC(10,2)`.
pub fn display_name(pg_type: &PgType, oid: u32, modifier: i32) -> String {
    match oid {
        BPCHAR | VARCHAR if modifier > 4 => format!("{}({})", pg_type.name, modifier - 4),
        NUMERIC if modifier > 4 => {
            let packed = modifier - 4;
            format!("{}({},{})", pg_type.name, (packed >> 16) & 0xffff, packed & 0xffff)
        }
        BIT | VARBIT if modifier > 0 => format!("{}({modifier})", pg_type.name),
        _ => pg_type.name.clone(),
    }
}

/// Converts the server's text output to how the grid shows values. Bytea and PostGIS hex get a `0x` prefix.
pub fn display_value(value: Option<String>, category: ValueCategory) -> Option<String> {
    let value = value?;
    Some(match category {
        ValueCategory::Binary => match value.strip_prefix("\\x") {
            Some(hex) => format!("0x{}", hex.to_ascii_uppercase()),
            None => value,
        },
        ValueCategory::Spatial if value.chars().all(|character| character.is_ascii_hexdigit()) => {
            format!("0x{}", value.to_ascii_uppercase())
        }
        ValueCategory::Boolean => match value.as_str() {
            "t" => "true".to_string(),
            "f" => "false".to_string(),
            _ => value,
        },
        _ => value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_types_with_modifiers() {
        let varchar = known(VARCHAR).unwrap();
        assert_eq!(display_name(&varchar, VARCHAR, 44), "VARCHAR(40)");
        assert_eq!(display_name(&varchar, VARCHAR, -1), "VARCHAR");
        let numeric = known(NUMERIC).unwrap();
        assert_eq!(display_name(&numeric, NUMERIC, ((10 << 16) | 2) + 4), "NUMERIC(10,2)");
        assert_eq!(known(1007).unwrap().name, "INTEGER[]");
    }

    #[test]
    fn formats_values() {
        assert_eq!(display_value(Some("\\xdeadbeef".into()), ValueCategory::Binary).unwrap(), "0xDEADBEEF");
        assert_eq!(display_value(Some("t".into()), ValueCategory::Boolean).unwrap(), "true");
        assert_eq!(display_value(Some("0101".into()), ValueCategory::Spatial).unwrap(), "0x0101");
        assert_eq!(display_value(None, ValueCategory::Text), None);
    }

    #[test]
    fn classifies_catalog_types() {
        assert_eq!(classify("mood", "e", "E"), (ValueCategory::Text, true));
        assert_eq!(classify("geometry", "b", "U"), (ValueCategory::Spatial, false));
        assert_eq!(classify("citext", "b", "S"), (ValueCategory::Text, false));
    }
}
