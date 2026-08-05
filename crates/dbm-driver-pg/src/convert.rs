use dbm_core::{ColumnMeta, QueryResult, Row};
use tokio_postgres::types::Type;
use tokio_postgres::Row as PgRow;

pub fn format_pg_column_type(type_: &Type, type_modifier: i32) -> String {
    let name = type_.name().to_ascii_lowercase();
    if type_modifier < 0 {
        return name;
    }
    match name.as_str() {
        "varchar" | "bpchar" | "char" | "bit" | "varbit" => {
            let len = (type_modifier - 4).max(0) as u32;
            format!("{name}({len})")
        }
        "numeric" | "decimal" => {
            let m = (type_modifier - 4) as u32;
            let precision = m >> 16;
            let scale = m & 0xffff;
            if scale == 0 {
                format!("{name}({precision})")
            } else {
                format!("{name}({precision},{scale})")
            }
        }
        "time" | "timetz" | "timestamp" | "timestamptz" | "interval" => {
            let precision = (type_modifier - 4).max(0) as u32;
            if precision > 0 {
                format!("{name}({precision})")
            } else {
                name
            }
        }
        _ => name,
    }
}

pub fn rows_to_result(rows: &[PgRow]) -> QueryResult {
    if rows.is_empty() {
        return QueryResult {
            columns: vec![],
            rows: vec![],
            rows_affected: Some(0),
            total_rows: None,
        };
    }

    let columns = rows[0]
        .columns()
        .iter()
        .map(|col| ColumnMeta {
            name: col.name().to_string(),
            type_name: col.type_().name().to_string(),
            type_display: format_pg_column_type(col.type_(), col.type_modifier()),
            ..Default::default()
        })
        .collect();

    let converted_rows = rows.iter().map(row_to_values).collect();

    QueryResult {
        columns,
        rows: converted_rows,
        rows_affected: Some(rows.len() as u64),
        total_rows: None,
    }
}

fn row_to_values(row: &PgRow) -> Row {
    let values = (0..row.len()).map(|idx| cell_to_string(row, idx)).collect();
    Row { values }
}

fn cell_to_string(row: &PgRow, idx: usize) -> String {
    macro_rules! try_optional {
        ($ty:ty) => {
            if let Ok(value) = row.try_get::<_, Option<$ty>>(idx) {
                return match value {
                    Some(v) => v.to_string(),
                    None => "NULL".to_string(),
                };
            }
        };
    }

    try_optional!(String);
    try_optional!(i16);
    try_optional!(i32);
    try_optional!(i64);
    // PostgreSQL `"char"` (OID 18, e.g. pg_depend.deptype) maps to i8, not String.
    if let Ok(value) = row.try_get::<_, Option<i8>>(idx) {
        return match value {
            Some(v) => format_pg_char(v),
            None => "NULL".to_string(),
        };
    }
    try_optional!(bool);
    try_optional!(f32);
    try_optional!(f64);
    try_optional!(chrono::NaiveDate);
    try_optional!(chrono::NaiveDateTime);
    try_optional!(chrono::DateTime<chrono::Utc>);
    try_optional!(rust_decimal::Decimal);

    match row.try_get::<_, Option<&str>>(idx) {
        Ok(Some(v)) => v.to_string(),
        Ok(None) => "NULL".to_string(),
        Err(_) => format!("<{}>", row.columns()[idx].type_().name()),
    }
}

/// Display PostgreSQL `"char"` (OID 18, single byte) as its unsigned code value.
fn format_pg_char(v: i8) -> String {
    (v as u8).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_postgres::types::Type;

    #[test]
    fn format_varchar_with_length() {
        assert_eq!(
            format_pg_column_type(&Type::VARCHAR, 255 + 4),
            "varchar(255)"
        );
    }

    #[test]
    fn format_text_without_modifier() {
        assert_eq!(format_pg_column_type(&Type::TEXT, -1), "text");
    }

    #[test]
    fn format_numeric_precision_scale() {
        let type_mod = 4 + ((10 << 16) | 2);
        assert_eq!(
            format_pg_column_type(&Type::NUMERIC, type_mod),
            "numeric(10,2)"
        );
    }

    #[test]
    fn format_pg_char_as_unsigned_code() {
        assert_eq!(format_pg_char(b'n' as i8), "110");
        assert_eq!(format_pg_char(b'a' as i8), "97");
        assert_eq!(format_pg_char(b' ' as i8), "32");
        assert_eq!(format_pg_char(0), "0");
        assert_eq!(format_pg_char(0x0a), "10");
        assert_eq!(format_pg_char(-1), "255");
    }
}
