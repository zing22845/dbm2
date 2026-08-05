//! Discovery targets editor feature update.

use dbm_discovery::{parse_port_spec, parse_targets_tsv, validate_host};

use super::msg::TargetsMessage;
use super::state::{TargetCol, TargetRow, TargetsState};
use super::intent::TargetsIntent;
use super::effect::TargetsEffect;

/// Update the targets editor state. Pure by-value transition: the caller moves
/// the state in and receives the new state back.
pub fn update(
    msg: TargetsMessage,
    mut state: TargetsState,
) -> (TargetsState, Vec<TargetsIntent>, Vec<TargetsEffect>) {
    match msg {
        TargetsMessage::MoveUp => {
            state.row = state.row.saturating_sub(1);
        }
        TargetsMessage::MoveDown => {
            state.row = (state.row + 1).min(state.targets.len().saturating_sub(1));
        }
        TargetsMessage::MoveColHost => state.col = TargetCol::Host,
        TargetsMessage::MoveColPorts => state.col = TargetCol::Ports,
        TargetsMessage::AddRow => {
            // Refuse to add a row while any row is incomplete.
            if state.has_empty_row() {
                return (state, Vec::new(), Vec::new());
            }
            push_undo(&mut state);
            let insert_at = (state.row + 1).min(state.targets.len());
            state.targets.insert(
                insert_at,
                TargetRow {
                    host: String::new(),
                    ports_spec: String::new(),
                },
            );
            state.row = insert_at;
            state.col = TargetCol::Host;
        }
        TargetsMessage::DeleteRow => {
            if state.targets.len() > 1 {
                push_undo(&mut state);
                state.targets.remove(state.row);
                state.row = state.row.min(state.targets.len().saturating_sub(1));
            }
        }
        TargetsMessage::BeginEdit => {
            if let Some(row) = state.targets.get(state.row) {
                state.editing = true;
                state.edit_buf = match state.col {
                    TargetCol::Host => row.host.clone(),
                    TargetCol::Ports => row.ports_spec.clone(),
                };
                state.edit_cursor = state.edit_buf.len();
            }
        }
        TargetsMessage::CommitEdit => commit_edit(&mut state),
        TargetsMessage::CancelEdit => state.discard_edit(),
        TargetsMessage::EditChar(c) if !c.is_control() => {
            if state.editing {
                state.edit_buf.insert(state.edit_cursor, c);
                state.edit_cursor += c.len_utf8();
            }
        }
        TargetsMessage::EditChar(_) => {}
        TargetsMessage::EditBackspace => {
            if state.editing {
                let prefix = &state.edit_buf[..state.edit_cursor];
                if let Some(c) = prefix.chars().next_back() {
                    let start = state.edit_cursor - c.len_utf8();
                    state.edit_buf.drain(start..state.edit_cursor);
                    state.edit_cursor = start;
                }
            }
        }
        TargetsMessage::EditCursorLeft => {
            if state.editing {
                let prefix = &state.edit_buf[..state.edit_cursor];
                if let Some(c) = prefix.chars().next_back() {
                    state.edit_cursor -= c.len_utf8();
                }
            }
        }
        TargetsMessage::EditCursorRight => {
            if state.editing {
                let rest = &state.edit_buf[state.edit_cursor..];
                if let Some(c) = rest.chars().next() {
                    state.edit_cursor += c.len_utf8();
                }
            }
        }
        TargetsMessage::Undo => undo_targets(&mut state),
        TargetsMessage::Redo => redo_targets(&mut state),
        TargetsMessage::Paste(contents) => {
            if state.editing {
                state.edit_buf.insert_str(state.edit_cursor, &contents);
                state.edit_cursor += contents.len();
            } else {
                paste_targets(&mut state, &contents);
            }
        }
        TargetsMessage::CommitCell { row, col, value } => {
            if let Some(r) = state.targets.get_mut(row) {
                match col {
                    TargetCol::Host => r.host = value,
                    TargetCol::Ports => r.ports_spec = value,
                }
            }
        }
    }
    (state, Vec::new(), Vec::new())
}

fn push_undo(state: &mut TargetsState) {
    state.undo_stack.push(state.targets.clone());
    // A fresh mutation invalidates the redo history.
    state.redo_stack.clear();
}

fn undo_targets(state: &mut TargetsState) {
    if let Some(prev) = state.undo_stack.pop() {
        state.redo_stack.push(std::mem::replace(&mut state.targets, prev));
        state.row = state.row.min(state.targets.len().saturating_sub(1));
    }
}

fn redo_targets(state: &mut TargetsState) {
    if let Some(next) = state.redo_stack.pop() {
        state.undo_stack.push(std::mem::replace(&mut state.targets, next));
        state.row = state.row.min(state.targets.len().saturating_sub(1));
    }
}

/// Commit the in-progress edit, validating the value before writing it back.
fn commit_edit(state: &mut TargetsState) {
    let value = state.edit_buf.clone();
    let value = match state.col {
        TargetCol::Host => validate_host(&value),
        TargetCol::Ports => parse_port_spec(&value).map(|_| value.trim().to_string()),
    };
    let Ok(value) = value else {
        // Validation failed: keep editing so the user can correct the value.
        return;
    };
    push_undo(state);
    if let Some(row) = state.targets.get_mut(state.row) {
        match state.col {
            TargetCol::Host => row.host = value,
            TargetCol::Ports => row.ports_spec = value,
        }
    }
    state.discard_edit();
}

/// Paste TSV target rows onto the end of the list. Strict: a malformed row
/// rejects the whole batch.
fn paste_targets(state: &mut TargetsState, contents: &str) {
    let Ok(rows) = parse_targets_tsv(contents) else {
        return;
    };
    if rows.is_empty() {
        return;
    }
    push_undo(state);
    state.targets.extend(rows.into_iter().map(|(host, ports_spec)| TargetRow {
        host,
        ports_spec,
    }));
    state.row = state.targets.len().saturating_sub(1);
}
