//! Reusable TEA UI components (list, input, tabs, ...).
//!
//! Self-contained components that follow the TEA pattern and can be embedded
//! by features, so common widgets are not re-implemented per feature.

/// Unified in-pane `/` search (state, input, matching, title suffix).
pub mod search;

/// Absolute line-number gutter helpers (edtui-aligned width formula).
pub mod line_numbers;
