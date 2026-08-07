//! Discovery targets editor feature update.

use dbm_discovery::{parse_port_spec, parse_targets_tsv, validate_host};

use super::msg::TargetsMessage;
use super::state::{TargetCol, TargetRow, TargetsState};
use super::intent::TargetsIntent;
use super::effect::TargetsEffect;

/// Update the targets editor state. Pure by-value transition: the caller moves
/// the state in and receives the new state back.
///
/// The returned `bool` is `dirty`: whether the rendered targets editor changed.
/// Navigation and edits report `false` when they are no-ops (e.g. moving past a
/// boundary, or an edit when not editing).
pub fn update(
    msg: TargetsMessage,
    mut state: TargetsState,
) -> (TargetsState, Vec<TargetsIntent>, Vec<TargetsEffect>, bool) {
    let dirty = match msg {
        TargetsMessage::MoveUp => {
            let before = state.row;
            state.row = state.row.saturating_sub(1);
            state.row != before
        }
        TargetsMessage::MoveDown => {
            let before = state.row;
            state.row = (state.row + 1).min(state.targets.len().saturating_sub(1));
            state.row != before
        }
        TargetsMessage::MoveColHost => {
            let changed = state.col != TargetCol::Host;
            state.col = TargetCol::Host;
            changed
        }
        TargetsMessage::MoveColPorts => {
            let changed = state.col != TargetCol::Ports;
            state.col = TargetCol::Ports;
            changed
        }
        TargetsMessage::AddRow => {
            // Refuse to add a row while any row is incomplete.
            if state.has_empty_row() {
                return (state, Vec::new(), Vec::new(), false);
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
            true
        }
        TargetsMessage::DeleteRow => {
            if state.targets.len() > 1 {
                push_undo(&mut state);
                state.targets.remove(state.row);
                state.row = state.row.min(state.targets.len().saturating_sub(1));
                true
            } else {
                false
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
                true
            } else {
                false
            }
        }
        TargetsMessage::CommitEdit => commit_edit(&mut state),
        TargetsMessage::CancelEdit => {
            let changed = state.editing;
            state.discard_edit();
            changed
        }
        TargetsMessage::EditChar(c) if !c.is_control() => {
            if state.editing {
                state.edit_buf.insert(state.edit_cursor, c);
                state.edit_cursor += c.len_utf8();
                true
            } else {
                false
            }
        }
        TargetsMessage::EditChar(_) => false,
        TargetsMessage::EditBackspace => {
            if state.editing {
                let prefix = &state.edit_buf[..state.edit_cursor];
                if let Some(c) = prefix.chars().next_back() {
                    let start = state.edit_cursor - c.len_utf8();
                    state.edit_buf.drain(start..state.edit_cursor);
                    state.edit_cursor = start;
                    true
                } else {
                    false
                }
            } else {
                false
            }
        }
        TargetsMessage::EditCursorLeft => {
            if state.editing {
                let prefix = &state.edit_buf[..state.edit_cursor];
                if let Some(c) = prefix.chars().next_back() {
                    state.edit_cursor -= c.len_utf8();
                    true
                } else {
                    false
                }
            } else {
                false
            }
        }
        TargetsMessage::EditCursorRight => {
            if state.editing {
                let rest = &state.edit_buf[state.edit_cursor..];
                if let Some(c) = rest.chars().next() {
                    state.edit_cursor += c.len_utf8();
                    true
                } else {
                    false
                }
            } else {
                false
            }
        }
        TargetsMessage::Undo => undo_targets(&mut state),
        TargetsMessage::Redo => redo_targets(&mut state),
        TargetsMessage::Paste(contents) => {
            if state.editing {
                state.edit_buf.insert_str(state.edit_cursor, &contents);
                state.edit_cursor += contents.len();
                true
            } else {
                paste_targets(&mut state, &contents)
            }
        }
        TargetsMessage::CommitCell { row, col, value } => {
            if let Some(r) = state.targets.get_mut(row) {
                match col {
                    TargetCol::Host => r.host = value,
                    TargetCol::Ports => r.ports_spec = value,
                }
                true
            } else {
                false
            }
        }
    };
    (state, Vec::new(), Vec::new(), dirty)
}

fn push_undo(state: &mut TargetsState) {
    state.undo_stack.push(state.targets.clone());
    // A fresh mutation invalidates the redo history.
    state.redo_stack.clear();
}

fn undo_targets(state: &mut TargetsState) -> bool {
    if let Some(prev) = state.undo_stack.pop() {
        state.redo_stack.push(std::mem::replace(&mut state.targets, prev));
        state.row = state.row.min(state.targets.len().saturating_sub(1));
        true
    } else {
        false
    }
}

fn redo_targets(state: &mut TargetsState) -> bool {
    if let Some(next) = state.redo_stack.pop() {
        state.undo_stack.push(std::mem::replace(&mut state.targets, next));
        state.row = state.row.min(state.targets.len().saturating_sub(1));
        true
    } else {
        false
    }
}

/// Commit the in-progress edit, validating the value before writing it back.
/// Returns whether the edit was actually applied.
fn commit_edit(state: &mut TargetsState) -> bool {
    let value = state.edit_buf.clone();
    let value = match state.col {
        TargetCol::Host => validate_host(&value),
        TargetCol::Ports => parse_port_spec(&value).map(|_| value.trim().to_string()),
    };
    let Ok(value) = value else {
        // Validation failed: keep editing so the user can correct the value.
        return false;
    };
    push_undo(state);
    if let Some(row) = state.targets.get_mut(state.row) {
        match state.col {
            TargetCol::Host => row.host = value,
            TargetCol::Ports => row.ports_spec = value,
        }
    }
    state.discard_edit();
    true
}

/// Paste TSV target rows onto the end of the list. Strict: a malformed row
/// rejects the whole batch. Returns whether any rows were added.
fn paste_targets(state: &mut TargetsState, contents: &str) -> bool {
    let Ok(rows) = parse_targets_tsv(contents) else {
        return false;
    };
    if rows.is_empty() {
        return false;
    }
    push_undo(state);
    state.targets.extend(rows.into_iter().map(|(host, ports_spec)| TargetRow {
        host,
        ports_spec,
    }));
    state.row = state.targets.len().saturating_sub(1);
    true
}
