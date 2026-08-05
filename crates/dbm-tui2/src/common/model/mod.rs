//! Pure-data shared model enums.
//!
//! Shared model types that multiple features reference but no single feature
//! owns. Kept deliberately small and TEA-neutral.

/// Row-change classification shared between results editing and theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowChangeKind {
    NoChange,
    Insert,
    Delete,
    Update,
}
