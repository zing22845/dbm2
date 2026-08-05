//! Results feature update.
//!
//! Pure by-value transition over the result set, cell selection, `/` search
//! and detail sub-pane. Query execution is a deferred effect (not yet wired).

use crate::common::components::search::PaneSearchInput;

use super::msg::ResultsMessage;
use super::state::ResultsState;
use super::intent::ResultsIntent;
use super::effect::ResultsEffect;
use super::detail;

pub fn update(
    msg: ResultsMessage,
    mut state: ResultsState,
) -> (ResultsState, Vec<ResultsIntent>, Vec<ResultsEffect>) {
    let mut intents = Vec::new();
    let mut effects = Vec::new();
    match msg {
        ResultsMessage::SetResult { result, paginated } => {
            state.result = Some(result);
            state.paginated = paginated;
            state.row = 0;
            state.col = 0;
            state.h_scroll = 0;
            state.search.reset();
            state.detail.scroll = 0;
        }
        ResultsMessage::ClearResult => {
            state.result = None;
            state.row = 0;
            state.col = 0;
            state.detail.scroll = 0;
        }
        ResultsMessage::MoveSelection { dr, dc } => {
            if state.move_selection(dr, dc) {
                state.detail.scroll = 0;
            }
        }
        ResultsMessage::BeginSearch => {
            state.search.reset();
            state.search.start();
        }
        ResultsMessage::SearchKey(key) => {
            handle_search_key(&mut state, key);
        }
        ResultsMessage::ResetSelection => {
            state.row = 0;
            state.col = 0;
            state.h_scroll = 0;
            state.search.reset();
            state.detail.scroll = 0;
        }
        ResultsMessage::Detail(m) => {
            let detail::msg::DetailMsg::Message(inner) = m;
            let detail_state = std::mem::take(&mut state.detail);
            let (s, i, e) = detail::update::update(inner, detail_state);
            state.detail = s;
            intents.extend(i.into_iter().map(ResultsIntent::Detail));
            effects.extend(e.into_iter().map(ResultsEffect::Detail));
        }
    }
    (state, intents, effects)
}

fn handle_search_key(state: &mut ResultsState, key: crossterm::event::KeyEvent) {
    let caps_lock = false;
    let action = match key.code {
        crossterm::event::KeyCode::Esc => {
            state.search.reset();
            PaneSearchInput::Cancelled
        }
        crossterm::event::KeyCode::Enter => {
            state.search.end();
            PaneSearchInput::Applied
        }
        _ => state.search.handle_key(&key, caps_lock),
    };

    if let PaneSearchInput::Navigate { forward } = action {
        let _ = state.move_selection(if forward { 1 } else { -1 }, 0);
        return;
    }
    if matches!(action, PaneSearchInput::QueryChanged | PaneSearchInput::OptionsChanged) {
        state.row = 0;
    }
}
