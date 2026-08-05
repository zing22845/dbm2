use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    Postgres,
}

impl Engine {
    /// Two-letter ASCII badge for tree/list UIs. Fixed width, terminal-safe (no emoji/icon fonts).
    pub fn tree_badge(self) -> &'static str {
        match self {
            Self::Postgres => "PG",
        }
    }
}

impl fmt::Display for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Postgres => write!(f, "postgres"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_badge_is_ascii_and_fixed_width() {
        assert_eq!(Engine::Postgres.tree_badge(), "PG");
        assert_eq!(Engine::Postgres.tree_badge().len(), 2);
    }
}
