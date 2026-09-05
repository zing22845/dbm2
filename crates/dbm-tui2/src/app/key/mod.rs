//! Keyboard input forwarding.
//!
//! The run loop reads raw key events; global shortcuts are handled there, and
//! everything else is handed to [`key_to_msg`], which maps a key to a feature
//! message. When a modal is open it owns all keys; otherwise the key is routed
//! by the active focus pane. The children are split by pane, mirroring the
//! `mouse` module: [`dispatch`] routes one key to the pane handler that owns
//! its focus ([`nav`], [`header`], [`explorer`], [`discover`], [`iw`],
//! [`sql`], or the modal overlay [`modal`]).

mod discover;
mod dispatch;
mod explorer;
mod header;
mod iw;
mod modal;
mod nav;
mod sql;

pub use dispatch::{key_to_msg, paste_to_msg};
