//! Discovery targets editor feature update.

use dbm_discovery::{parse_port_spec, parse_targets_lines_lenient, validate_host};

use super::effect::TargetsEffect;
use super::intent::TargetsIntent;
use super::msg::TargetsMessage;
use super::state::{TargetCol, TargetRow, TargetsState};

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
    // `last_paste_content` guards against a held Cmd+V re-feeding the exact
    // same payload as consecutive paste events. Any non-paste action (deleting
    // the pasted rows, editing, undoing, …) ends that run, so a later paste of
    // the same text is treated as a fresh action instead of being swallowed.
    let is_paste = matches!(&msg, TargetsMessage::Paste(_));
    let dirty = match msg {
        // Manual scroll (scrollbar drag): lock the viewport so cursor anchor
        // does not override the dragged position until the next cursor move.
        TargetsMessage::SetVScroll { position } => {
            let total = state.targets.len();
            let max = total.saturating_sub(state.target_viewport.max(1));
            let new = position.min(max);
            let changed = state.scroll_offset != new;
            state.scroll_offset = new;
            state.scroll_locked = true;
            changed
        }
        TargetsMessage::MoveUp => {
            let before = state.row;
            state.row = state.row.saturating_sub(1);
            let changed = state.row != before;
            if changed {
                state.scroll_locked = false;
                state.ensure_row_visible(state.row);
            }
            changed
        }
        TargetsMessage::MoveDown => {
            let before = state.row;
            state.row = (state.row + 1).min(state.targets.len().saturating_sub(1));
            let changed = state.row != before;
            if changed {
                state.scroll_locked = false;
                state.ensure_row_visible(state.row);
            }
            changed
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
            state.clamp_scroll();
            state.ensure_row_visible(state.row);
            true
        }
        TargetsMessage::DeleteRow => {
            if state.targets.len() > 1 {
                push_undo(&mut state);
                state.targets.remove(state.row);
                state.row = state.row.min(state.targets.len().saturating_sub(1));
                state.clamp_scroll();
                state.ensure_row_visible(state.row);
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
        TargetsMessage::Undo => {
            // A list change marks dirty; an empty undo stack still sets the
            // "Nothing to undo" status, so repaint only when that status is
            // newly shown (holding `u` must not repaint every repeat).
            let before = state.status.clone();
            let dirty = undo_targets(&mut state);
            state.clamp_scroll();
            state.ensure_row_visible(state.row);
            dirty || state.status != before
        }
        TargetsMessage::Redo => {
            let before = state.status.clone();
            let dirty = redo_targets(&mut state);
            state.clamp_scroll();
            state.ensure_row_visible(state.row);
            dirty || state.status != before
        }
        TargetsMessage::Paste(contents) => {
            if state.editing {
                state.edit_buf.insert_str(state.edit_cursor, &contents);
                state.edit_cursor += contents.len();
                true
            } else {
                let dirty = paste_targets(&mut state, &contents);
                state.clamp_scroll();
                state.ensure_row_visible(state.row);
                dirty
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
        TargetsMessage::SelectRow { row } => {
            if state.editing {
                commit_edit(&mut state);
            }
            let row = row.min(state.targets.len().saturating_sub(1));
            let changed = state.row != row;
            state.row = row;
            state.scroll_locked = false;
            if changed {
                state.ensure_row_visible(state.row);
            }
            changed
        }
        TargetsMessage::SelectCell { row, col } => {
            if state.editing {
                commit_edit(&mut state);
            }
            let row = row.min(state.targets.len().saturating_sub(1));
            let changed = state.row != row || state.col != col;
            state.row = row;
            state.col = col;
            state.scroll_locked = false;
            if state.row != row {
                state.ensure_row_visible(state.row);
            }
            changed
        }
        TargetsMessage::BeginEditCell { row, col } => {
            if state.editing {
                commit_edit(&mut state);
            }
            let row = row.min(state.targets.len().saturating_sub(1));
            let mut changed = state.row != row || state.col != col;
            state.row = row;
            state.col = col;
            state.scroll_locked = false;
            if changed {
                state.ensure_row_visible(state.row);
            }
            if let Some(r) = state.targets.get(state.row) {
                state.editing = true;
                state.edit_buf = match state.col {
                    TargetCol::Host => r.host.clone(),
                    TargetCol::Ports => r.ports_spec.clone(),
                };
                state.edit_cursor = state.edit_buf.len();
                changed = true;
            }
            changed
        }
    };
    if !is_paste {
        // A non-paste action ends the current paste de-dup run (see `is_paste`
        // above), so a later paste of the same text is not swallowed.
        state.last_paste_content = None;
    }
    (state, Vec::new(), Vec::new(), dirty)
}

fn push_undo(state: &mut TargetsState) {
    state.undo_stack.push(state.targets.clone());
    // A fresh mutation invalidates the redo history.
    state.redo_stack.clear();
}

fn undo_targets(state: &mut TargetsState) -> bool {
    if let Some(prev) = state.undo_stack.pop() {
        state
            .redo_stack
            .push(std::mem::replace(&mut state.targets, prev));
        state.row = state.row.min(state.targets.len().saturating_sub(1));
        state.status = Some(format!("Undone {} target(s)", state.targets.len()));
        true
    } else {
        state.status = Some("Nothing to undo".into());
        false
    }
}

fn redo_targets(state: &mut TargetsState) -> bool {
    if let Some(next) = state.redo_stack.pop() {
        state
            .undo_stack
            .push(std::mem::replace(&mut state.targets, next));
        state.row = state.row.min(state.targets.len().saturating_sub(1));
        state.status = Some(format!("Redone {} target(s)", state.targets.len()));
        true
    } else {
        state.status = Some("Nothing to redo".into());
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

/// Paste TSV target rows onto the end of the list. Lenient: each line is parsed
/// independently, so valid rows are added while bad rows are skipped (unlike
/// the strict [`parse_targets_tsv`], which rejects the whole batch). The result
/// is reported on the targets footer status line.
///
/// De-duplicates like the original dbm: (1) re-feeding the exact same payload
/// (a held Cmd+V auto-repeat) is ignored, and (2) a pasted `host:ports` that
/// already exists in the list is not appended twice.
///
/// Returns `dirty` as a single unified rule: `true` iff the target list **or**
/// the status line changed. A held Cmd+V therefore repaints only on the first
/// repeat (status flips to "Duplicate paste ignored"); further repeats change
/// neither, so they repaint nothing (no redraw storm).
fn paste_targets(state: &mut TargetsState, contents: &str) -> bool {
    let before_status = state.status.clone();
    let before_len = state.targets.len();

    // Re-fed identical payload (a held Cmd+V) -> ignore, just set the status.
    if state.last_paste_content.as_deref() == Some(contents) {
        state.status = Some("Duplicate paste ignored".into());
        return state.targets.len() != before_len || state.status != before_status;
    }

    let lines = parse_targets_lines_lenient(contents);
    let total = lines.len();
    if total == 0 {
        state.status = Some("Nothing to paste".into());
        return state.targets.len() != before_len || state.status != before_status;
    }

    let mut added = 0usize;
    let mut duplicated = 0usize;
    let mut failed = 0usize;
    let mut new_rows = Vec::new();
    for line in lines {
        match line {
            Ok((host, ports_spec)) => {
                let row = TargetRow { host, ports_spec };
                if state.targets.contains(&row) {
                    duplicated += 1;
                } else {
                    added += 1;
                    new_rows.push(row);
                }
            }
            Err(_) => failed += 1,
        }
    }
    state.last_paste_content = Some(contents.to_string());
    state.status = Some(format!(
        "Paste: {added}/{total} added, {duplicated}/{total} duplicated, {failed}/{total} failed"
    ));
    if !new_rows.is_empty() {
        push_undo(state);
        state.targets.extend(new_rows);
        state.row = state.targets.len().saturating_sub(1);
    }
    state.targets.len() != before_len || state.status != before_status
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paste(state: &mut TargetsState, contents: &str) -> bool {
        let (s, _i, _e, dirty) = update(
            TargetsMessage::Paste(contents.to_string()),
            std::mem::take(state),
        );
        *state = s;
        dirty
    }

    #[test]
    fn re_fed_identical_paste_is_de_duped() {
        let mut s = TargetsState::with_default_targets();
        let first = paste(&mut s, "db.example.com\t5432\n");
        assert!(first);
        let before = s.targets.len();
        // Re-feeding the exact same payload (held Cmd+V) appends nothing: the
        // list length is unchanged (the dirty flag is about status feedback,
        // covered by `duplicate_paste_ignored_reports_status`).
        paste(&mut s, "db.example.com\t5432\n");
        assert_eq!(s.targets.len(), before);
    }

    #[test]
    fn paste_drops_duplicate_targets() {
        let mut s = TargetsState::with_default_targets();
        // First paste adds db.example.com:5432.
        paste(&mut s, "db.example.com\t5432\n");
        assert!(s.targets.iter().any(|r| r.host == "db.example.com"));
        let before = s.targets.len();
        // A *different* payload that repeats db.example.com:5432 keeps only the
        // new rows; the existing one is not appended again.
        paste(&mut s, "db.example.com\t5432\nother.example.com\t5433\n");
        assert!(s.targets.iter().any(|r| r.host == "other.example.com"));
        assert_eq!(s.targets.len(), before + 1);
    }

    #[test]
    fn paste_status_counts_added_duplicated_and_failed() {
        let mut s = TargetsState::with_default_targets();
        // 127.0.0.1 / 5432,5433-5440 is the default row, so repeating it
        // de-duplicates; `bad host` fails validation; `db.example.com:5432` is
        // added.
        paste(
            &mut s,
            "127.0.0.1\t5432,5433-5440\nbad host\t5433\ndb.example.com:5432\n",
        );
        let status = s.status.as_deref().expect("paste sets a status");
        assert!(status.contains("1/3 added"), "{status}");
        assert!(status.contains("1/3 duplicated"), "{status}");
        assert!(status.contains("1/3 failed"), "{status}");
    }

    #[test]
    fn duplicate_paste_ignored_reports_status() {
        let mut s = TargetsState::with_default_targets();
        paste(&mut s, "db.example.com\t5432\n");
        // The first re-fed identical payload shows "Duplicate paste ignored"
        // and repaints once so the feedback is visible.
        let dirty = paste(&mut s, "db.example.com\t5432\n");
        assert!(
            dirty,
            "first duplicate paste must repaint to show the status"
        );
        assert_eq!(s.status.as_deref(), Some("Duplicate paste ignored"));
        // A further held-Cmd+V repeat keeps the same status, so it must NOT
        // repaint again (no redraw storm).
        let dirty = paste(&mut s, "db.example.com\t5432\n");
        assert!(!dirty);
        assert_eq!(s.status.as_deref(), Some("Duplicate paste ignored"));
    }

    #[test]
    fn paste_that_only_duplicates_still_marks_dirty_for_status() {
        let mut s = TargetsState::with_default_targets();
        paste(&mut s, "db.example.com\t5432\n");
        // A non-paste action (cursor move) ends the held-Cmd+V run, so the same
        // payload is treated as a fresh paste whose rows all already exist:
        // nothing is added, but the status line must be refreshed so the
        // feedback is visible.
        let (s1, _i, _e, _d) = update(TargetsMessage::MoveDown, std::mem::take(&mut s));
        s = s1;
        let dirty = paste(&mut s, "db.example.com\t5432\n");
        assert!(
            dirty,
            "a fresh paste that only duplicates must repaint the status"
        );
        let status = s.status.as_deref().expect("paste sets a status");
        assert!(status.contains("0/1 added"), "{status}");
        assert!(status.contains("1/1 duplicated"), "{status}");
    }

    #[test]
    fn empty_undo_status_is_dirty_only_once() {
        let mut s = TargetsState::with_default_targets();
        // First undo on an empty stack shows "Nothing to undo" and repaints.
        let (s1, _i, _e, dirty) = update(TargetsMessage::Undo, std::mem::take(&mut s));
        s = s1;
        assert!(dirty);
        assert_eq!(s.status.as_deref(), Some("Nothing to undo"));
        // Repeating undo keeps the same status, so it must not repaint.
        let (_s2, _i, _e, dirty) = update(TargetsMessage::Undo, std::mem::take(&mut s));
        assert!(!dirty);
    }

    #[test]
    fn undo_and_redo_set_status_counts() {
        let mut s = TargetsState::with_default_targets();
        paste(&mut s, "db.example.com\t5432\n");
        // Undo the paste -> back to one target.
        let (s1, _i, _e, dirty) = update(TargetsMessage::Undo, std::mem::take(&mut s));
        s = s1;
        assert!(dirty);
        assert!(s.status.as_deref().is_some_and(|t| t.contains("Undone")));
        // Redo -> target list grows again.
        let (s2, _i, _e, dirty) = update(TargetsMessage::Redo, std::mem::take(&mut s));
        s = s2;
        assert!(dirty);
        assert!(s.status.as_deref().is_some_and(|t| t.contains("Redone")));
    }

    #[test]
    fn repaste_after_deleting_pasted_rows_is_allowed() {
        let mut s = TargetsState::with_default_targets();
        let payload = "db.example.com\t5432\nother.example.com\t5433\n";
        // First paste adds both rows.
        assert!(paste(&mut s, payload));
        assert_eq!(s.targets.len(), 3);
        // Delete both pasted rows (from the end back to the top) — a non-paste
        // action, which must reset the paste de-dup guard.
        let (s1, _i, _e, _d) = update(TargetsMessage::DeleteRow, std::mem::take(&mut s));
        s = s1;
        let (s2, _i, _e, _d) = update(TargetsMessage::DeleteRow, std::mem::take(&mut s));
        s = s2;
        assert_eq!(s.targets.len(), 1);
        // Re-pasting the identical payload is a fresh action now, not a held
        // Cmd+V repeat, so both rows are added again.
        assert!(paste(&mut s, payload));
        assert_eq!(s.targets.len(), 3);
    }

    #[test]
    fn repaste_after_deleting_some_rows_restores_only_missing_ones() {
        let mut s = TargetsState::with_default_targets();
        let payload = "db.example.com\t5432\nother.example.com\t5433\n";
        // First paste adds both rows; the cursor moves to the last row
        // (`other.example.com`), and the default loopback row stays.
        paste(&mut s, payload);
        assert_eq!(s.targets.len(), 3);
        // Delete just one pasted row (the cursor is on `other.example.com`).
        // Deleting a single row is a non-paste action, so it resets the de-dup
        // guard; `db.example.com` remains in the list.
        let (s1, _i, _e, _d) = update(TargetsMessage::DeleteRow, std::mem::take(&mut s));
        s = s1;
        assert_eq!(s.targets.len(), 2);
        // Re-pasting the full payload restores only the missing row; the rows
        // still in the list are de-duplicated, not appended again.
        assert!(paste(&mut s, payload));
        assert_eq!(s.targets.len(), 3);
        let status = s.status.as_deref().expect("paste sets a status");
        assert!(status.contains("1/2 added"), "{status}");
        assert!(status.contains("1/2 duplicated"), "{status}");
        assert!(status.contains("0/2 failed"), "{status}");
    }

    #[test]
    fn empty_undo_and_redo_report_not_available() {
        let mut s = TargetsState::with_default_targets();
        let (s1, _i, _e, _dirty) = update(TargetsMessage::Undo, std::mem::take(&mut s));
        s = s1;
        assert_eq!(s.status.as_deref(), Some("Nothing to undo"));
        let (s2, _i, _e, _dirty) = update(TargetsMessage::Redo, std::mem::take(&mut s));
        s = s2;
        assert_eq!(s.status.as_deref(), Some("Nothing to redo"));
    }
}
