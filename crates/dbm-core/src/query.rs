use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub columns: Vec<ColumnMeta>,
    pub rows: Vec<Row>,
    pub rows_affected: Option<u64>,
    /// Total row count for paginated SELECT results (`COUNT(*)` over the user query).
    #[serde(default)]
    pub total_rows: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ColumnMeta {
    pub name: String,
    pub type_name: String,
    /// Human-readable type including length/precision when known (e.g. `varchar(255)`).
    #[serde(default)]
    pub type_display: String,
    /// Column comment from catalog (`pg_description`), when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Row {
    pub values: Vec<String>,
}

impl QueryResult {
    pub fn empty_message(message: impl Into<String>) -> Self {
        Self {
            columns: vec![ColumnMeta {
                name: "result".into(),
                type_name: "text".into(),
                ..Default::default()
            }],
            rows: vec![Row {
                values: vec![message.into()],
            }],
            rows_affected: None,
            total_rows: None,
        }
    }
}
