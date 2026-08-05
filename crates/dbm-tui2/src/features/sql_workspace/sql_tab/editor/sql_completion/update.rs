//! SQL completion sub-module update.

use super::msg::SqlCompletionMessage;
use super::state::SqlCompletionState;
use super::intent::SqlCompletionIntent;
use super::effect::SqlCompletionEffect;

pub fn update(
    _msg: SqlCompletionMessage,
    _state: SqlCompletionState,
) -> (SqlCompletionState, Vec<SqlCompletionIntent>, Vec<SqlCompletionEffect>) {
    match _msg {}
}
