//! SQL workspace feature update.

use super::msg::SqlMessage;
use super::state::SqlState;
use super::intent::SqlIntent;
use super::effect::SqlEffect;
use super::sql_tab;

/// Update the SQL workspace state. Delegates `SqlTab` messages to the
/// `sql_tab` child feature by moving the child state in and out, so only the
/// touched sub-tree is carried; nothing here is deep-cloned per message.
pub fn update(
    msg: SqlMessage,
    mut state: SqlState,
) -> (SqlState, Vec<SqlIntent>, Vec<SqlEffect>, bool) {
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    let dirty = match msg {
        SqlMessage::SqlTab(tab_msg) => {
            let sql_tab::msg::SqlTabMsg::Message(inner) = tab_msg;
            let tab_state = std::mem::take(&mut state.sql_tab);
            let (s, tab_intents, tab_effects, d) = sql_tab::update::update(inner, tab_state);
            state.sql_tab = s;
            // Re-wrap child intents/effects into this feature's types so they
            // remain in the same side-channel stream.
            intents.extend(tab_intents.into_iter().map(SqlIntent::SqlTab));
            effects.extend(tab_effects.into_iter().map(SqlEffect::SqlTab));
            d
        }
    };
    (state, intents, effects, dirty)
}
