//! SQL completion sub-module messages.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SqlCompletionMessage {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SqlCompletionMsg {
    Message(SqlCompletionMessage),
}

impl From<SqlCompletionMessage> for SqlCompletionMsg {
    fn from(m: SqlCompletionMessage) -> Self {
        SqlCompletionMsg::Message(m)
    }
}
