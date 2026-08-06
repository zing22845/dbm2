//! The `Intent` trait. An intent is a request emitted by a feature's `update`
//! to be delivered to another (or the same) feature. The associated `Message`
//! type is the feature message that the router will deliver, and it must
//! convert into the global `AppMsg` via `From`.
//!
//! This module defines two concepts:
//! - `Intent`: the *business* trait that each feature implements. It describes
//!   "given this request, what concrete feature message should be produced?".
//!   Business developers only ever need to implement this trait.
//! - `RoutableIntent`: the *infrastructure* type-erasure layer. It is only used
//!   by the central router (`UpdateResult`, `IntentRouter`) to store
//!   heterogeneous intents in a single `Vec` and route them uniformly. Business
//!   code does not need to reference it directly.

/// An intent carries a target feature message and is routable.
///
/// Implement this on a feature's `*Intent` enum. Each variant should map to a
/// single concrete `Message` (1:1 request); if a feature needs to fan out to
/// multiple targets, produce multiple intents from its `update` instead.
///
/// `into_message` returns `None` for *cross-feature* intents: requests that are
/// routed explicitly by a parent feature (e.g. the SQL editor's `RunQuery` is
/// intercepted by `sql_tab`, which owns the connection context). Such variants
/// must not be re-dispatched into their own sub-module, so they explicitly
/// decline to produce a message and the router skips them instead of panicking.
pub trait Intent: Send + 'static {
    /// The feature message this intent will be converted into.
    type Message: Send + 'static;

    /// Convert this intent into the concrete feature message, or `None` when
    /// the intent is routed by a parent feature and has no sub-module message.
    fn into_message(self) -> Option<Self::Message>;
}

/// A type-erased intent that resolves directly to the global message type `M`
/// (typically `AppMsg`).
///
/// This is the erasure layer that lets the central router hold intents from
/// every feature in one heterogeneous `Vec<Box<dyn RoutableIntent<M>>>` and
/// route them without knowing their concrete types. It is automatically
/// implemented for any `Intent` whose message converts into `M`; business code
/// should not implement it directly.
pub trait RoutableIntent<M>: Send {
    /// Convert the erased intent into the global message type, or `None` when
    /// the intent has no message to dispatch (a cross-feature request that the
    /// router should skip).
    fn into_global_message(self: Box<Self>) -> Option<M>;
}

impl<I, Msg, M> RoutableIntent<M> for I
where
    I: Intent<Message = Msg> + 'static,
    Msg: Into<M> + 'static,
    M: 'static,
{
    fn into_global_message(self: Box<Self>) -> Option<M> {
        I::into_message(*self).map(Into::into)
    }
}
