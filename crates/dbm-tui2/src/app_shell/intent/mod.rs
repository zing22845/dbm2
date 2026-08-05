//! Intent subsystem: features express "requests to other features" as
//! `Intent`s, which are routed by `IntentRouter` back into the central
//! `AppMsg` router.

pub mod intent;
pub mod intent_trait;
pub mod router;

pub use intent::ShellIntent;
pub use intent_trait::{Intent, RoutableIntent};
pub use router::IntentRouter;
