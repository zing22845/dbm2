//! The intent router. It receives type-erased `RoutableIntent`s (produced by
//! the central router's `UpdateResult`) and converts each back into the global
//! message type `M`, feeding it to the update loop. A more elaborate
//! implementation could apply routing rules / filtering.

use super::intent_trait::RoutableIntent;

/// Routes erased intents (resolved to global message `M`) into the update loop.
#[derive(Debug, Default, Clone)]
pub struct IntentRouter;

impl IntentRouter {
    /// Convert an erased intent into the global message type, or `None` when
    /// the intent has no message to dispatch (the router should skip it).
    pub fn route<M: Send + 'static>(intent: Box<dyn RoutableIntent<M>>) -> Option<M> {
        intent.into_global_message()
    }
}
