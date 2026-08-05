/// Quote a PostgreSQL identifier for safe interpolation into DDL/session SQL.
pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}
