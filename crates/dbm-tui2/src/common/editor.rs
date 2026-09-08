//! Shared SQL editor helpers (wrappers around the vendored `edtui` editor).
//!
//! The editor is the foundational leaf: it owns the SQL buffer and cursor via
//! `edtui::EditorState`. This module provides feature-agnostic helpers
//! (construction, rendering, hardware cursor, key normalization, paste) that
//! the editor feature and its children (completion, context picker) reuse.

use crossterm::ExecutableCommand;
use crossterm::cursor::SetCursorStyle;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use edtui::clipboard::InternalClipboard;
use edtui::{
    EditorMode, EditorState, EditorTheme, EditorView, Index2, LineNumbers, Lines, SyntaxHighlighter,
};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Clear, Widget};
use std::io;

use crate::common::utils::shortcuts::{caps_lock_active, effective_ascii_letter};

fn editor_contains_non_ascii(editor: &EditorState) -> bool {
    editor
        .lines
        .iter_row()
        .any(|line| line.iter().any(|ch| !ch.is_ascii()))
}

pub fn new_editor(default_sql: &str) -> EditorState {
    // `Lines::from("")` yields 0 rows (str::lines() iterates nothing), so an empty
    // editor would render no line-number gutter until the first character is typed.
    // Guarantee at least one (empty) row so the gutter always shows "1".
    let lines = if default_sql.is_empty() {
        Lines::new(vec![Vec::new()])
    } else {
        Lines::from(default_sql)
    };
    let mut editor = EditorState::new(lines);
    // Keep yank/paste buffers in-process. edtui's default ArboardClipboard round-trips
    // through the OS clipboard on set+get, which truncates multi-byte UTF-8 (e.g. 中文 → 中).
    editor.set_clipboard(InternalClipboard::default());
    move_cursor_to_eol(&mut editor);
    editor
}

/// Place the cursor after the last character on the last line (Insert-mode EOL).
pub fn move_cursor_to_eol(editor: &mut EditorState) {
    let last_row = editor.lines.last_row_index();
    let last_col = editor.lines.len_col(last_row).unwrap_or(0);
    editor.cursor = Index2::new(last_row, last_col);
}

/// Normal-mode `d{motion}`: visual selection through motion, then delete.
fn delete_through_motion(motion: impl Into<edtui::actions::Action>) -> edtui::actions::Action {
    use edtui::actions::{Chainable, DeleteSelection, SwitchMode};
    SwitchMode(EditorMode::Visual)
        .chain(motion)
        .chain(DeleteSelection)
        .chain(SwitchMode(EditorMode::Normal))
        .into()
}

fn delete_inner_word() -> edtui::actions::Action {
    use edtui::actions::{Chainable, DeleteSelection, SelectInnerWord};
    SelectInnerWord.chain(DeleteSelection).into()
}

fn register_normal_delete_operator(key_handler: &mut edtui::events::KeyEventHandler) {
    use edtui::actions::{
        DeleteWordBackward, DeleteWordForward, MoveBackward, MoveDown, MoveForward,
        MoveParagraphBackward, MoveParagraphForward, MoveToEndOfLine, MoveToFirst,
        MoveToStartOfLine, MoveUp, MoveWordForwardToEndOfWord,
    };
    use edtui::events::{KeyEventRegister, KeyInput};

    key_handler.insert(
        KeyEventRegister::n(vec![KeyInput::new('d'), KeyInput::new('w')]),
        DeleteWordForward(1),
    );
    key_handler.insert(
        KeyEventRegister::n(vec![KeyInput::new('d'), KeyInput::new('e')]),
        delete_through_motion(MoveWordForwardToEndOfWord(1)),
    );
    key_handler.insert(
        KeyEventRegister::n(vec![KeyInput::new('d'), KeyInput::new('b')]),
        DeleteWordBackward(1),
    );
    key_handler.insert(
        KeyEventRegister::n(vec![
            KeyInput::new('d'),
            KeyInput::new('i'),
            KeyInput::new('w'),
        ]),
        delete_inner_word(),
    );
    key_handler.insert(
        KeyEventRegister::n(vec![KeyInput::new('d'), KeyInput::new('h')]),
        delete_through_motion(MoveBackward(1)),
    );
    key_handler.insert(
        KeyEventRegister::n(vec![KeyInput::new('d'), KeyInput::new('l')]),
        delete_through_motion(MoveForward(1)),
    );
    key_handler.insert(
        KeyEventRegister::n(vec![KeyInput::new('d'), KeyInput::new('j')]),
        delete_through_motion(MoveDown(1)),
    );
    key_handler.insert(
        KeyEventRegister::n(vec![KeyInput::new('d'), KeyInput::new('k')]),
        delete_through_motion(MoveUp(1)),
    );
    key_handler.insert(
        KeyEventRegister::n(vec![KeyInput::new('d'), KeyInput::new('0')]),
        delete_through_motion(MoveToStartOfLine()),
    );
    key_handler.insert(
        KeyEventRegister::n(vec![KeyInput::new('d'), KeyInput::new('$')]),
        delete_through_motion(MoveToEndOfLine()),
    );
    key_handler.insert(
        KeyEventRegister::n(vec![KeyInput::new('d'), KeyInput::new('_')]),
        delete_through_motion(MoveToFirst()),
    );
    key_handler.insert(
        KeyEventRegister::n(vec![KeyInput::new('d'), KeyInput::new('}')]),
        delete_through_motion(MoveParagraphForward()),
    );
    key_handler.insert(
        KeyEventRegister::n(vec![KeyInput::new('d'), KeyInput::new('{')]),
        delete_through_motion(MoveParagraphBackward()),
    );
}

/// Vim-style handler plus readline insert-mode keys (`CTRL+a/e`, etc.).
///
/// edtui `vim_mode` only maps Home/End for line motion in insert; emacs/readline
/// chords live in `emacs_mode`. Merge the common insert bindings here without
/// switching the editor out of vim normal/visual mode.
pub fn new_editor_handler() -> edtui::EditorEventHandler {
    use edtui::actions::delete::DeleteToEndOfLine;
    use edtui::actions::{
        DeleteChar, MoveBackward, MoveDown, MoveForward, MoveToEndOfLine, MoveToStartOfLine, MoveUp,
    };
    use edtui::events::{KeyEventHandler, KeyEventRegister, KeyInput};

    let mut key_handler = KeyEventHandler::vim_mode();
    key_handler.insert(
        KeyEventRegister::i(vec![KeyInput::ctrl('a')]),
        MoveToStartOfLine(),
    );
    key_handler.insert(
        KeyEventRegister::i(vec![KeyInput::ctrl('e')]),
        MoveToEndOfLine(),
    );
    key_handler.insert(
        KeyEventRegister::i(vec![KeyInput::ctrl('f')]),
        MoveForward(1),
    );
    key_handler.insert(
        KeyEventRegister::i(vec![KeyInput::ctrl('b')]),
        MoveBackward(1),
    );
    key_handler.insert(KeyEventRegister::i(vec![KeyInput::ctrl('p')]), MoveUp(1));
    key_handler.insert(KeyEventRegister::i(vec![KeyInput::ctrl('n')]), MoveDown(1));
    key_handler.insert(
        KeyEventRegister::i(vec![KeyInput::ctrl('h')]),
        DeleteChar(1),
    );
    key_handler.insert(
        KeyEventRegister::i(vec![KeyInput::ctrl('k')]),
        DeleteToEndOfLine,
    );
    register_normal_delete_operator(&mut key_handler);
    edtui::EditorEventHandler::new(key_handler)
}

/// Drop any in-progress vim key sequence (e.g. pending `d` delete operator).
///
/// `KeyEventHandler` stores partial chords in a shared handler. If the user
/// presses `d` in Normal mode then switches pane/focus before the motion key,
/// the next `h`/`j`/`k`/`l` can execute `dh`/`dj`/… and delete text unexpectedly.
pub fn reset_key_sequence(handler: &mut edtui::EditorEventHandler) {
    *handler = new_editor_handler();
}

/// Full SQL text of the editor (joined lines with `\n`).
pub fn sql_text(editor: &EditorState) -> String {
    editor.lines.to_string()
}

/// Alias for shared call sites (SQL + Detail).
pub fn editor_text(editor: &EditorState) -> String {
    sql_text(editor)
}

/// Replace the whole buffer and reset the editor to Normal mode.
pub fn set_sql_text(editor: &mut EditorState, sql: &str) {
    set_editor_text(editor, sql);
    editor.mode = EditorMode::Normal;
    editor.selection = None;
}

/// Replace buffer text; leaves mode/selection to the caller.
pub fn set_editor_text(editor: &mut EditorState, text: &str) {
    *editor = new_editor("");
    if text.is_empty() {
        return;
    }
    editor.lines = Lines::from(text);
    move_cursor_to_eol(editor);
    editor.selection = None;
}

/// Reset Detail / shared editor to Normal on enter (no Visual-only downgrade).
pub fn apply_normal_on_enter(editor: &mut EditorState) {
    editor.mode = EditorMode::Normal;
    editor.selection = None;
}

/// Orange style for Detail draft chars that differ from baseline.
pub fn detail_dirty_style() -> Style {
    Style::default().fg(Color::Rgb(255, 140, 0))
}

/// Highlight spans in `current` that differ from `baseline` (line-wise
/// prefix/suffix). Ported verbatim from the original dbm so the Detail draft
/// diff markers look identical.
pub fn detail_dirty_highlights(baseline: &str, current: &str) -> Vec<edtui::Highlight> {
    use edtui::{Highlight, Index2};
    if current == baseline {
        return Vec::new();
    }
    let style = detail_dirty_style();
    let base_lines: Vec<&str> = baseline.split('\n').collect();
    let cur_lines: Vec<&str> = current.split('\n').collect();
    let mut out = Vec::new();
    for (row, cur) in cur_lines.iter().enumerate() {
        let base = base_lines.get(row).copied().unwrap_or("");
        if *cur == base {
            continue;
        }
        if cur.is_empty() {
            continue;
        }
        let cur_chars: Vec<char> = cur.chars().collect();
        let base_chars: Vec<char> = base.chars().collect();
        let mut prefix = 0usize;
        while prefix < cur_chars.len()
            && prefix < base_chars.len()
            && cur_chars[prefix] == base_chars[prefix]
        {
            prefix += 1;
        }
        let mut suffix = 0usize;
        while suffix < cur_chars.len().saturating_sub(prefix)
            && suffix < base_chars.len().saturating_sub(prefix)
            && cur_chars[cur_chars.len() - 1 - suffix] == base_chars[base_chars.len() - 1 - suffix]
        {
            suffix += 1;
        }
        let start = prefix;
        let end = cur_chars.len().saturating_sub(suffix);
        if start < end {
            out.push(Highlight::new(
                Index2::new(row, start),
                Index2::new(row, end.saturating_sub(1)),
                style,
            ));
        } else if cur_chars.len() > base_chars.len() {
            // Insertion with empty changed middle — highlight trailing new chars.
            let from = base_chars.len().min(cur_chars.len());
            if from < cur_chars.len() {
                out.push(Highlight::new(
                    Index2::new(row, from),
                    Index2::new(row, cur_chars.len() - 1),
                    style,
                ));
            }
        } else if *cur != base {
            // Full-line fallback.
            out.push(Highlight::new(
                Index2::new(row, 0),
                Index2::new(row, cur_chars.len().saturating_sub(1)),
                style,
            ));
        }
    }
    out
}

/// Refresh the baseline-vs-draft diff highlights on `editor`.
pub fn refresh_detail_dirty_highlights(editor: &mut EditorState, baseline: &str) {
    let current = editor_text(editor);
    editor.set_highlights(detail_dirty_highlights(baseline, &current));
}

/// Theme for the results Detail cell editor: transparent base (so it blends
/// into the results pane palette), visible block cursor drawn into the buffer
/// (the Detail editor has no dedicated hardware-cursor slot), no status line.
fn detail_editor_theme() -> EditorTheme<'static> {
    EditorTheme::default()
        .base(Style::default())
        .selection_style(Style::default().bg(Color::DarkGray).fg(Color::White))
        .line_numbers_style(Style::default().fg(Color::DarkGray))
        .cursor_style(Style::default().bg(Color::White).fg(Color::Black))
        .hide_status_line()
}

/// Render the results Detail cell editor (plain text, no SQL highlight) with
/// its own block cursor. Unlike the SQL editor there is no hardware-cursor
/// plumbing here, so edtui paints the caret cell itself.
///
/// Returns the rendered mouse hit region like [`render_editor`] does: the text
/// area is `area` minus the line-number gutter, and `viewport_y` is the offset
/// the rendered copy actually drew with (its auto-scroll may have nudged it).
/// The run loop feeds it back so pointer clicks map onto the draft buffer.
pub fn render_detail_editor(
    editor: &mut EditorState,
    area: Rect,
    buf: &mut ratatui::buffer::Buffer,
) -> Option<EditorMouseHitArea> {
    if area.width == 0 || area.height == 0 {
        return None;
    }
    Clear.render(area, buf);
    EditorView::new(editor)
        .theme(detail_editor_theme())
        .wrap(true)
        .line_numbers(LineNumbers::Absolute)
        .render(area, buf);
    let gutter_w = editor_line_number_gutter_width(editor);
    let (_, rendered_viewport_y) = editor.viewport_offset();
    Some(EditorMouseHitArea {
        text_area: Rect {
            x: area.x.saturating_add(gutter_w),
            y: area.y,
            width: area.width.saturating_sub(gutter_w),
            height: area.height,
        },
        viewport_y: rendered_viewport_y,
    })
}

/// Strip Ctrl/Alt/Meta/Super/Hyper modifiers, keeping only Shift (and the code).
fn strip_non_text_modifiers(key: KeyEvent) -> KeyEvent {
    let mut modifiers = key.modifiers;
    modifiers.remove(
        KeyModifiers::CONTROL
            | KeyModifiers::ALT
            | KeyModifiers::META
            | KeyModifiers::SUPER
            | KeyModifiers::HYPER,
    );
    KeyEvent {
        code: key.code,
        modifiers,
        kind: key.kind,
        state: key.state,
    }
}

/// Terminals often send Shift+letter or Caps Lock+letter as lowercase char plus
/// modifier/state flags (especially on macOS with CSI-u). edtui inserts the char
/// literally, so normalize before forwarding to the editor.
///
/// IME commits for CJK and other non-ASCII text often arrive with Alt/Ctrl/Meta;
/// edtui insert mode only accepts unmodified or Shift-modified keys.
fn normalize_editor_key_event(key: KeyEvent, tracked_caps_lock: bool) -> KeyEvent {
    let KeyCode::Char(c) = key.code else {
        return key;
    };

    if !c.is_ascii() {
        return strip_non_text_modifiers(key);
    }

    let shift = key.modifiers.intersects(KeyModifiers::SHIFT);
    let caps_lock = caps_lock_active(&key, tracked_caps_lock);

    if (!shift && !caps_lock) || !c.is_ascii_alphabetic() {
        return key;
    }

    let mut modifiers = key.modifiers;
    modifiers.remove(KeyModifiers::SHIFT);

    KeyEvent {
        code: KeyCode::Char(effective_ascii_letter(c, shift, caps_lock)),
        modifiers,
        kind: key.kind,
        state: key.state,
    }
}

/// Only forward keys edtui understands (avoids panics on exotic key codes).
pub fn accepts_key_event(key: &KeyEvent) -> bool {
    matches!(
        key.code,
        KeyCode::Char(_)
            | KeyCode::Enter
            | KeyCode::Esc
            | KeyCode::Backspace
            | KeyCode::Delete
            | KeyCode::Tab
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown
    )
}

/// Insert `text` at the cursor via per-char insertion (Insert mode).
pub fn insert_text(handler: &mut edtui::EditorEventHandler, editor: &mut EditorState, text: &str) {
    for ch in text.chars() {
        handler.on_key_event(
            KeyEvent::new(KeyCode::Char(ch), KeyModifiers::empty()),
            editor,
        );
    }
}

/// Insert bracketed-paste or other bulk Unicode at the cursor.
pub fn paste_text(handler: &mut edtui::EditorEventHandler, editor: &mut EditorState, text: &str) {
    if text.is_empty() {
        return;
    }
    if editor.mode == EditorMode::Insert {
        insert_text(handler, editor, text);
    } else {
        handler.on_paste_event(text.to_string(), editor);
    }
}

/// Captures every mutable field of `EditorState` that matters for redraw decisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorStateSnapshot {
    pub text: String,
    pub cursor: Index2,
    pub mode: EditorMode,
    selection_hash: u64,
}

impl EditorStateSnapshot {
    pub fn from_editor(editor: &EditorState) -> Self {
        let text = sql_text(editor);
        let cursor = editor.cursor;
        let mode = editor.mode;
        let selection_hash = hash_selection(&editor.selection);
        Self {
            text,
            cursor,
            mode,
            selection_hash,
        }
    }

    pub fn matches_full(&self, editor: &EditorState) -> bool {
        self.cursor == editor.cursor
            && self.mode == editor.mode
            && self.selection_hash == hash_selection(&editor.selection)
            && self.text == sql_text(editor)
    }

    pub fn matches_nav(&self, editor: &EditorState) -> bool {
        self.cursor == editor.cursor
            && self.mode == editor.mode
            && self.selection_hash == hash_selection(&editor.selection)
    }
}

fn hash_selection(sel: &Option<impl std::fmt::Debug>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    format!("{sel:?}").hash(&mut h);
    h.finish()
}

fn editor_theme() -> EditorTheme<'static> {
    EditorTheme::default()
        .base(Style::default())
        .selection_style(Style::default().bg(Color::DarkGray).fg(Color::White))
        .line_numbers_style(Style::default().fg(Color::DarkGray))
        .hide_status_line()
        .hide_cursor()
}

/// Absolute line-number gutter width (edtui `LineNumbers::Absolute` formula).
pub fn editor_line_number_gutter_width(editor: &EditorState) -> u16 {
    let total_lines = editor.lines.len().max(1);
    let digits = total_lines.to_string().len();
    (digits + 1) as u16
}

fn editor_wrap_width(editor: &EditorState, area_width: u16) -> u16 {
    area_width
        .saturating_sub(editor_line_number_gutter_width(editor))
        .max(1)
}

/// Terminal cursor shape for each editor mode (hardware cursor, not buffer-painted).
pub fn hardware_cursor_style(mode: EditorMode) -> SetCursorStyle {
    match mode {
        EditorMode::Insert => SetCursorStyle::SteadyBar,
        EditorMode::Normal | EditorMode::Visual => SetCursorStyle::SteadyBlock,
        EditorMode::Search => SetCursorStyle::SteadyUnderScore,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorHardwareCursor {
    pub position: Position,
    pub style: SetCursorStyle,
}

/// The editor's mouse hit region, mirroring what the last render drew.
///
/// edtui converts terminal coordinates to buffer positions from the editor's
/// own `screen_area` + viewport, which its renderer only refreshes on the
/// `&mut` editor it draws. dbm2 renders a *copy* each frame, so the run loop
/// captures these two values from the render copy and stores them here; the
/// pointer handlers feed them back into a scratch editor before calling
/// edtui's mouse handler, keeping the mapping identical to what is on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorMouseHitArea {
    /// The text-drawing area (after the line-number gutter and scrollbar) in
    /// absolute terminal coordinates — edtui's `screen_area`.
    pub text_area: Rect,
    /// The first visible *visual* (wrapped) row as actually rendered.
    pub viewport_y: usize,
}

impl EditorMouseHitArea {
    /// Whether `point` lands on the editor's text area (mouse hit region).
    #[must_use]
    pub fn contains(&self, point: Position) -> bool {
        let area = self.text_area;
        point.x >= area.x
            && point.y >= area.y
            && point.x < area.x.saturating_add(area.width)
            && point.y < area.y.saturating_add(area.height)
    }
}

/// Keep the hardware cursor inside `area` (edtui wrap can report a y below the
/// viewport for long single lines).
pub fn clamp_hardware_cursor_to_area(
    cursor: EditorHardwareCursor,
    area: Rect,
) -> EditorHardwareCursor {
    if area.width == 0 || area.height == 0 {
        return cursor;
    }
    let max_x = area.x.saturating_add(area.width.saturating_sub(1));
    let max_y = area.y.saturating_add(area.height.saturating_sub(1));
    EditorHardwareCursor {
        position: Position {
            x: cursor.position.x.clamp(area.x, max_x),
            y: cursor.position.y.clamp(area.y, max_y),
        },
        style: cursor.style,
    }
}

/// Reset the terminal cursor to the user default (call on TUI teardown).
pub fn reset_hardware_cursor() -> io::Result<()> {
    io::stdout()
        .execute(SetCursorStyle::DefaultUserShape)
        .map(|_| ())
}

/// Insert-mode click placement: a click on the empty cells right of the text
/// (or past the wrapped last row) must land the cursor *after* the last
/// character (`col == len`), not on the last character itself.
///
/// edtui's mouse handler clamps a click beyond the end of a line to the last
/// character index, so appending text would insert before the final char.
/// Mirrors the original dbm's `fix_insert_mode_click_cursor`. Applied on a
/// scratch editor copy before its cursor/mode/selection are read back, so the
/// fix never runs mid-edit on the live buffer.
pub fn fix_insert_mode_click_cursor(
    editor: &mut EditorState,
    event: &crossterm::event::MouseEvent,
    area: Rect,
) {
    use crossterm::event::{MouseButton, MouseEventKind};
    if editor.mode != EditorMode::Insert || area.width == 0 {
        return;
    }
    if !matches!(
        event.kind,
        MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left)
    ) {
        return;
    }
    if event.column < area.x
        || event.row < area.y
        || event.column >= area.x.saturating_add(area.width)
        || event.row >= area.y.saturating_add(area.height)
    {
        return;
    }

    let len_col = editor.lines.len_col(editor.cursor.row).unwrap_or(0);
    if len_col == 0 {
        return;
    }
    let last_char_index = len_col.saturating_sub(1);
    if editor.cursor.col != last_char_index {
        return;
    }

    let Some(line) = editor.lines.iter_row().nth(editor.cursor.row) else {
        return;
    };
    let line_display_width: u16 = line
        .iter()
        .map(|ch| crate::common::utils::text_width::char_width(*ch) as u16)
        .sum();
    let click_col = event.column.saturating_sub(area.x);
    let fits_one_row = line_display_width <= area.width;
    if (fits_one_row && click_col >= line_display_width)
        || (!fits_one_row && click_col.saturating_add(1) >= area.width)
    {
        editor.cursor.col = len_col;
    }
}

/// Move the terminal hardware caret to the editor cursor and set its style.
/// `None` (editor not focused / not visible) hides the caret so it does not
/// linger on the wrong cell after switching away from the editor.
pub fn apply_hardware_cursor(cursor: Option<EditorHardwareCursor>) -> io::Result<()> {
    use crossterm::cursor::{Hide, MoveTo, Show};
    let mut out = io::stdout();
    match cursor {
        Some(c) => {
            let pos = c.position;
            out.execute(Show)?;
            out.execute(MoveTo(pos.x, pos.y))?;
            out.execute(c.style)?;
        }
        None => {
            out.execute(Hide)?;
        }
    }
    Ok(())
}

/// Wrapped display rows across all logical editor lines at `area_width`.
pub fn editor_display_row_count(editor: &EditorState, area_width: u16) -> usize {
    let text_width = editor_wrap_width(editor, area_width);
    if text_width == 0 {
        return 0;
    }
    let w = text_width as usize;
    editor
        .lines
        .iter_row()
        .map(|line| wrapped_row_count(&editor_line_string(line), w))
        .sum()
}

pub fn editor_max_v_scroll(editor: &EditorState, text_width: u16, viewport_height: u16) -> usize {
    let total = editor_display_row_count(editor, text_width);
    total.saturating_sub(viewport_height.max(1) as usize)
}

pub fn editor_v_scroll_display(editor: &EditorState, _text_width: u16) -> usize {
    editor.viewport_offset().1
}

pub fn editor_set_v_scroll_display(editor: &mut EditorState, _text_width: u16, display_row: usize) {
    let (x, _) = editor.viewport_offset();
    editor.set_viewport_offset(x, display_row);
}

pub fn clamp_editor_v_scroll(editor: &mut EditorState, text_width: u16, viewport_height: u16) {
    let max = editor_max_v_scroll(editor, text_width, viewport_height);
    let current = editor_v_scroll_display(editor, text_width);
    if current > max {
        editor_set_v_scroll_display(editor, text_width, max);
    }
}

/// Render the editor into `buf`, returning the hardware-cursor position.
pub fn render_editor(
    editor: &mut EditorState,
    area: Rect,
    buf: &mut ratatui::buffer::Buffer,
) -> Option<EditorHardwareCursor> {
    render_editor_with_sql_highlight(editor, area, buf, true)
}

/// Detail / plain text: no SQL syntax highlighter.
pub fn render_editor_plain(
    editor: &mut EditorState,
    area: Rect,
    buf: &mut ratatui::buffer::Buffer,
) -> Option<EditorHardwareCursor> {
    render_editor_with_sql_highlight(editor, area, buf, false)
}

fn render_editor_with_sql_highlight(
    editor: &mut EditorState,
    area: Rect,
    buf: &mut ratatui::buffer::Buffer,
    sql_highlight: bool,
) -> Option<EditorHardwareCursor> {
    // Shorter lines do not overwrite trailing cells; explicit clear avoids stale
    // wide-character columns after backspace.
    Clear.render(area, buf);

    let highlighter = if sql_highlight && !editor_contains_non_ascii(editor) {
        SyntaxHighlighter::new("base16-ocean.dark", "sql").ok()
    } else {
        None
    };
    EditorView::new(editor)
        .theme(editor_theme())
        .wrap(true)
        .line_numbers(LineNumbers::Absolute)
        .syntax_highlighter(highlighter)
        .render(area, buf);

    Some(EditorHardwareCursor {
        position: editor.cursor_screen_position()?,
        style: hardware_cursor_style(editor.mode),
    })
}

fn editor_line_string(line: &[char]) -> String {
    line.iter().collect()
}

/// Number of wrapped display rows a single logical line occupies at `width`.
fn wrapped_row_count(line: &str, width: usize) -> usize {
    if line.is_empty() {
        return 1;
    }
    let mut rows = 1usize;
    let mut used = 0usize;
    for ch in line.chars() {
        let cw = crate::common::utils::text_width::char_width(ch);
        if used > 0 && used + cw > width {
            rows += 1;
            used = cw;
        } else {
            used += cw;
        }
    }
    rows
}

/// Insert a single non-ASCII character in Insert mode, bypassing edtui key routing.
pub fn try_insert_non_ascii_key(
    handler: &mut edtui::EditorEventHandler,
    editor: &mut EditorState,
    key: KeyEvent,
    tracked_caps_lock: bool,
) -> bool {
    if editor.mode != EditorMode::Insert {
        return false;
    }
    let KeyCode::Char(c) = key.code else {
        return false;
    };
    if c.is_ascii() {
        return false;
    }
    let key = normalize_editor_key_event(key, tracked_caps_lock);
    if !key.modifiers.is_empty() && !key.modifiers.intersects(KeyModifiers::SHIFT) {
        return false;
    }
    let KeyCode::Char(c) = key.code else {
        return false;
    };
    insert_text(handler, editor, &c.to_string());
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{
        KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, ModifierKeyCode,
    };

    #[test]
    fn shift_lowercase_becomes_uppercase_char() {
        let key = normalize_editor_key_event(
            KeyEvent {
                code: KeyCode::Char('h'),
                modifiers: KeyModifiers::SHIFT,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            },
            false,
        );
        assert_eq!(key.code, KeyCode::Char('H'));
        assert!(!key.modifiers.intersects(KeyModifiers::SHIFT));
    }

    #[test]
    fn caps_lock_state_uppercases_letter() {
        let key = normalize_editor_key_event(
            KeyEvent {
                code: KeyCode::Char('a'),
                modifiers: KeyModifiers::empty(),
                kind: KeyEventKind::Press,
                state: KeyEventState::CAPS_LOCK,
            },
            false,
        );
        assert_eq!(key.code, KeyCode::Char('A'));
    }

    #[test]
    fn tracked_caps_lock_uppercases_letter() {
        let key = normalize_editor_key_event(
            KeyEvent {
                code: KeyCode::Char('a'),
                modifiers: KeyModifiers::empty(),
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            },
            true,
        );
        assert_eq!(key.code, KeyCode::Char('A'));
    }

    #[test]
    fn caps_lock_and_shift_lowercases_letter() {
        let key = normalize_editor_key_event(
            KeyEvent {
                code: KeyCode::Char('a'),
                modifiers: KeyModifiers::SHIFT,
                kind: KeyEventKind::Press,
                state: KeyEventState::CAPS_LOCK,
            },
            false,
        );
        assert_eq!(key.code, KeyCode::Char('a'));
        assert!(!key.modifiers.intersects(KeyModifiers::SHIFT));
    }

    #[test]
    fn strips_alt_from_non_ascii_char() {
        let key = normalize_editor_key_event(
            KeyEvent {
                code: KeyCode::Char('中'),
                modifiers: KeyModifiers::ALT,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            },
            false,
        );
        assert_eq!(key.code, KeyCode::Char('中'));
        assert!(key.modifiers.is_empty());
    }

    #[test]
    fn rejects_modifier_only_keys() {
        let key = KeyEvent {
            code: KeyCode::Modifier(ModifierKeyCode::LeftControl),
            modifiers: KeyModifiers::empty(),
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        };
        assert!(!accepts_key_event(&key));
    }

    #[test]
    fn accepts_ctrl_char_combos() {
        let key = KeyEvent {
            code: KeyCode::Char('d'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        };
        assert!(accepts_key_event(&key));
    }

    #[test]
    fn insert_mode_ctrl_a_e_move_within_line() {
        let mut editor = new_editor("select 1");
        editor.mode = EditorMode::Insert;
        editor.cursor = Index2::new(0, 4);
        let mut handler = new_editor_handler();

        handler.on_key_event(
            KeyEvent {
                code: KeyCode::Char('a'),
                modifiers: KeyModifiers::CONTROL,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            },
            &mut editor,
        );
        assert_eq!(editor.cursor.col, 0);

        handler.on_key_event(
            KeyEvent {
                code: KeyCode::Char('e'),
                modifiers: KeyModifiers::CONTROL,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            },
            &mut editor,
        );
        assert_eq!(editor.cursor.col, editor.lines.len_col(0).unwrap_or(0));
    }

    fn normal_char_key(c: char) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::empty(),
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn press_normal_keys(
        handler: &mut edtui::EditorEventHandler,
        editor: &mut EditorState,
        keys: &str,
    ) {
        for c in keys.chars() {
            handler.on_key_event(normal_char_key(c), editor);
        }
    }

    #[test]
    fn normal_mode_dw_deletes_to_next_word() {
        let mut editor = new_editor("select foo");
        editor.mode = EditorMode::Normal;
        editor.cursor = Index2::new(0, 0);
        let mut handler = new_editor_handler();

        press_normal_keys(&mut handler, &mut editor, "dw");

        assert_eq!(sql_text(&editor), "foo");
        assert_eq!(editor.mode, EditorMode::Normal);
    }

    #[test]
    fn paste_text_stores_chinese_in_insert_mode() {
        let mut editor = new_editor("");
        editor.mode = EditorMode::Insert;
        let mut handler = edtui::EditorEventHandler::default();
        paste_text(&mut handler, &mut editor, "中文");
        assert_eq!(sql_text(&editor), "中文");
    }

    #[test]
    fn paste_text_stores_chinese_in_normal_mode() {
        let mut editor = new_editor("select ");
        editor.mode = EditorMode::Normal;
        editor.cursor = Index2::new(0, 7);
        let mut handler = edtui::EditorEventHandler::default();
        paste_text(&mut handler, &mut editor, "中文");
        assert_eq!(sql_text(&editor), "select 中文");
    }

    #[test]
    fn insert_text_stores_chinese() {
        let mut editor = new_editor("");
        editor.mode = edtui::EditorMode::Insert;
        let mut handler = edtui::EditorEventHandler::default();
        insert_text(&mut handler, &mut editor, "中文");
        assert_eq!(sql_text(&editor), "中文");
    }

    #[test]
    fn try_insert_non_ascii_key_inserts_in_insert_mode() {
        let mut editor = new_editor("");
        editor.mode = edtui::EditorMode::Insert;
        let mut handler = edtui::EditorEventHandler::default();
        let key = KeyEvent {
            code: KeyCode::Char('中'),
            modifiers: KeyModifiers::ALT,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        assert!(try_insert_non_ascii_key(
            &mut handler,
            &mut editor,
            key,
            false
        ));
        assert_eq!(sql_text(&editor), "中");
    }

    #[test]
    fn renders_chinese_characters() {
        let mut editor = new_editor("中文");
        let area = Rect::new(0, 0, 10, 3);
        let mut buf = ratatui::buffer::Buffer::empty(area);
        render_editor(&mut editor, area, &mut buf);

        let rendered = (0..area.height)
            .map(|y| {
                (0..area.width)
                    .filter_map(|x| buf.cell((x, y)).map(|c| c.symbol()))
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            rendered.contains('中'),
            "expected Chinese in buffer, got: {rendered:?}"
        );
    }

    #[test]
    fn hardware_cursor_style_per_mode() {
        assert_eq!(
            hardware_cursor_style(EditorMode::Insert),
            SetCursorStyle::SteadyBar
        );
        assert_eq!(
            hardware_cursor_style(EditorMode::Normal),
            SetCursorStyle::SteadyBlock
        );
        assert_eq!(
            hardware_cursor_style(EditorMode::Visual),
            SetCursorStyle::SteadyBlock
        );
        assert_eq!(
            hardware_cursor_style(EditorMode::Search),
            SetCursorStyle::SteadyUnderScore
        );
    }

    #[test]
    fn clamp_hardware_cursor_keeps_position_inside_area() {
        let area = Rect::new(10, 5, 20, 4);
        let cursor = EditorHardwareCursor {
            position: Position::new(50, 40),
            style: SetCursorStyle::SteadyBlock,
        };
        let clamped = clamp_hardware_cursor_to_area(cursor, area);
        assert_eq!(clamped.position, Position::new(29, 8));
    }
}
