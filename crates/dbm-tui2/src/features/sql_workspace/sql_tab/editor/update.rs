//! Editor feature update.

use super::msg::EditorMessage;
use super::state::EditorState;
use super::intent::EditorIntent;
use super::effect::EditorEffect;
use super::context_picker;
use super::sql_completion;

pub fn update(
    msg: EditorMessage,
    mut state: EditorState,
) -> (EditorState, Vec<EditorIntent>, Vec<EditorEffect>) {
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    match msg {
        EditorMessage::ContextPicker(m) => {
            let context_picker::msg::ContextPickerMsg::Message(inner) = m;
            let cp_state = std::mem::take(&mut state.context_picker);
            let (s, i, e) = context_picker::update::update(inner, cp_state);
            state.context_picker = s;
            intents.extend(i.into_iter().map(EditorIntent::ContextPicker));
            effects.extend(e.into_iter().map(EditorEffect::ContextPicker));
        }
        EditorMessage::SqlCompletion(m) => {
            let sql_completion::msg::SqlCompletionMsg::Message(inner) = m;
            let sc_state = std::mem::take(&mut state.sql_completion);
            let (s, i, e) = sql_completion::update::update(inner, sc_state);
            state.sql_completion = s;
            intents.extend(i.into_iter().map(EditorIntent::SqlCompletion));
            effects.extend(e.into_iter().map(EditorEffect::SqlCompletion));
        }
    }
    (state, intents, effects)
}
